//! CPU benchmarks for `accreta`, using criterion.
//!
//! Requires adding to Cargo.toml:
//!
//! ```toml
//! [dev-dependencies]
//! criterion = "0.5"
//!
//! [[bench]]
//! name = "engine_bench"
//! harness = false
//! ```
//!
//! Place this file at `benches/engine_bench.rs`. Run with `cargo bench`;
//! an HTML report lands at `target/criterion/report/index.html`.
//!
//! `bench_ingest` and `bench_rollup` use `Engine::new/ingest/rollup`, whose
//! exact signatures come from the crate's own doctest (`engine.rs` itself
//! wasn't available to verify against). `bench_aggregate_set_merge`,
//! `bench_bucket_merge`, and `bench_dimension_dictionary_lookup` are built
//! entirely from confirmed `aggregate_set.rs` / `bucket.rs` / `dimensions.rs`
//! signatures.

use std::hint::black_box;

use chrono::{Duration, TimeZone, Utc};
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use accreta::aggregate_set::Schema;
use accreta::aggregates::{Count, Sum};
use accreta::bucket::{Bucket, BucketLevel};
use accreta::dimensions::{DimensionDictionaries, DimensionDictionary, DimensionValues};
use accreta::engine::Engine;
use accreta::measures::{MeasureId, MeasureValue, MeasureValues};
use accreta::sample::Sample;

fn basic_schema() -> Schema {
    let mut builder = Schema::builder();
    builder
        .dimension("host")
        .measure("value")
        .with::<Sum<f64>>()
        .with_any::<Count>();
    builder.build().expect("schema should build")
}

// ---------------------------------------------------------------------
// Ingest throughput at increasing sample counts, all into one dimension
// group (isolates raw ingest cost from dimension-dictionary growth).
// ---------------------------------------------------------------------

fn bench_ingest(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_ingest");
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap();

    for &n in &[100usize, 1_000, 10_000] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| {
                let schema = basic_schema();
                let mut engine = Engine::new(schema);
                for i in 0..n {
                    let t = t0 + Duration::seconds(i as i64);
                    engine
                        .ingest(black_box(t), black_box([i as f64]), black_box(["server-a"]))
                        .unwrap();
                }
                engine
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------
// Rollup cost at increasing minute-bucket counts. Uses iter_batched so
// ingestion (setup) isn't counted, only rollup() itself.
// ---------------------------------------------------------------------

fn bench_rollup(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_rollup");
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap();

    for &n in &[60usize, 600, 3_600] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let schema = basic_schema();
                    let mut engine = Engine::new(schema);
                    for i in 0..n {
                        engine
                            .ingest(t0 + Duration::minutes(i as i64), [i as f64], ["server-a"])
                            .unwrap();
                    }
                    engine
                },
                |mut engine| {
                    engine.rollup();
                    engine
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------
// AggregateSet::merge microbenchmark (no Engine involved).
// ---------------------------------------------------------------------

fn bench_aggregate_set_merge(c: &mut Criterion) {
    let schema = basic_schema();
    let mut dictionaries = DimensionDictionaries::new(schema.dimension_count());
    let id = dictionaries.dictionaries[0].get_or_insert("server-a");

    let make_sample = |v: f64| {
        Sample::new(
            Utc::now(),
            MeasureValues::new(vec![MeasureValue::F64(v)]),
            DimensionValues::new(vec![id]),
        )
    };

    let mut a = schema.empty_set(MeasureId(0)).unwrap();
    a.update(&make_sample(1.0));
    let mut b = schema.empty_set(MeasureId(0)).unwrap();
    b.update(&make_sample(2.0));

    c.bench_function("aggregate_set_merge", |bencher| {
        bencher.iter(|| {
            let mut left = a.clone();
            left.merge(black_box(&b));
            left
        });
    });
}

// ---------------------------------------------------------------------
// Bucket::merge at increasing dimension-group counts (matching keys, so
// this measures in-place AggregateSet merges across many groups).
// ---------------------------------------------------------------------

fn bench_bucket_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("bucket_merge");
    let schema = basic_schema();
    let start = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();

    for &groups in &[10usize, 100, 1_000] {
        group.throughput(Throughput::Elements(groups as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(groups),
            &groups,
            |bencher, &groups| {
                let mut dictionaries = DimensionDictionaries::new(schema.dimension_count());

                let make_bucket = |dictionaries: &mut DimensionDictionaries, offset: usize| {
                    let mut bucket = Bucket::new(BucketLevel::Minute, start);
                    for i in 0..groups {
                        let id = dictionaries.dictionaries[0].get_or_insert(&format!("server-{i}"));
                        let s = Sample::new(
                            Utc::now(),
                            MeasureValues::new(vec![MeasureValue::F64((i + offset) as f64)]),
                            DimensionValues::new(vec![id]),
                        );
                        bucket.update(&s, &schema);
                    }
                    bucket
                };

                bencher.iter_batched(
                    || {
                        (
                            make_bucket(&mut dictionaries, 0),
                            make_bucket(&mut dictionaries, 1),
                        )
                    },
                    |(mut b1, b2)| {
                        b1.merge(black_box(&b2));
                        b1
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

// ---------------------------------------------------------------------
// DimensionDictionary lookup/insert cost at high cardinality.
// ---------------------------------------------------------------------

fn bench_dimension_dictionary_lookup(c: &mut Criterion) {
    let mut dict = DimensionDictionary::default();
    for i in 0..10_000 {
        dict.get_or_insert(&format!("value-{i}"));
    }

    c.bench_function("dimension_dictionary_get_hit", |b| {
        b.iter(|| dict.get(black_box("value-5000")));
    });

    c.bench_function("dimension_dictionary_get_or_insert_existing", |b| {
        b.iter(|| dict.get_or_insert(black_box("value-5000")));
    });
}

criterion_group!(
    benches,
    bench_ingest,
    bench_rollup,
    bench_aggregate_set_merge,
    bench_bucket_merge,
    bench_dimension_dictionary_lookup,
);
criterion_main!(benches);
