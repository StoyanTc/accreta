//! Memory-footprint tests for `accreta`.
//!
//! Installs a byte-counting allocator as this test binary's global
//! allocator. This only affects this file: each `tests/*.rs` file compiles
//! to its own binary, so the library and other test binaries are
//! unaffected.
//!
//! IMPORTANT: `cargo test` runs every `#[test]` fn in a binary on its own
//! thread, in parallel, by default. A process-wide byte counter can't tell
//! "my allocations" apart from "some other test's thread doing setup,
//! teardown, or stdout locking at the same moment" — so if each scenario
//! below were its own `#[test]`, their numbers would be polluted by
//! whichever other scenario happened to be running concurrently (this is
//! exactly what produced the impossible negative byte count and the
//! spurious "leak" the first time this file was run). There is deliberately
//! only ONE `#[test]` in this file — `memory_footprint_report` — which
//! calls each scenario in sequence. With a single test in the binary
//! there's nothing to run in parallel, so this holds regardless of
//! `--test-threads`. The `Mutex` below is now belt-and-suspenders rather
//! than load-bearing, but it's harmless to keep.
//!
//! These are NOT strict correctness tests — internal representations
//! (HashMap load factor, `Box<dyn ErasedState>` layout, etc.) aren't fully
//! known from the uploaded sources, so the bounds asserted below are
//! deliberately generous "don't regress by 10x" tripwires. Run with
//! `cargo test --release --test memory_footprint -- --nocapture` and read
//! the printed numbers for the real signal.
//!
//! Place this file at `tests/memory_footprint.rs`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{Duration, TimeZone, Utc};

use accreta::aggregate_set::Schema;
use accreta::aggregates::{Count, Sum};
use accreta::bucket::{Bucket, BucketLevel};
use accreta::dimensions::{DimensionDictionaries, DimensionValues};
use accreta::engine::Engine;
use accreta::measures::{MeasureId, MeasureValue, MeasureValues};
use accreta::sample::Sample;

// ---------------------------------------------------------------------
// Counting global allocator
// ---------------------------------------------------------------------

struct CountingAllocator;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::SeqCst);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size > layout.size() {
                let grew = new_size - layout.size();
                let now = CURRENT.fetch_add(grew, Ordering::SeqCst) + grew;
                PEAK.fetch_max(now, Ordering::SeqCst);
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Ordering::SeqCst);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Belt-and-suspenders: with only one `#[test]` in this binary (see the
/// module doc), there's nothing else to serialize against — but this
/// costs nothing to keep in case a scenario is ever split out again.
static MEASURE_LOCK: Mutex<()> = Mutex::new(());

/// Runs `f`, returning its result, the net change in currently-allocated
/// bytes, and the peak *excess* allocated (relative to the baseline
/// immediately before `f` ran) at any point during `f`.
fn measure<T>(f: impl FnOnce() -> T) -> (T, isize, usize) {
    let _guard = MEASURE_LOCK.lock().unwrap();
    let before = CURRENT.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let result = f();
    let after = CURRENT.load(Ordering::SeqCst);
    let peak = PEAK.load(Ordering::SeqCst);
    (
        result,
        after as isize - before as isize,
        peak.saturating_sub(before),
    )
}

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
// The single test entry point. Runs every scenario in sequence, on one
// thread, so the byte counts can't be polluted by another test's activity.
// Each scenario panics on its own failed assertion, so a failure here still
// points at exactly which scenario regressed.
// ---------------------------------------------------------------------

#[test]
fn memory_footprint_report() {
    baseline_schema_and_empty_engine_footprint();
    ingest_bytes_per_sample_single_group_is_bounded();
    ingest_bytes_per_unique_dimension_group_is_bounded();
    rollup_memory_overhead_is_small_relative_to_ingestion();
    repeated_aggregate_set_merges_do_not_leak();
    bucket_merge_bytes_scale_roughly_linearly_with_group_count();
}

// ---------------------------------------------------------------------
// 1. Baseline: schema + empty engine.
// ---------------------------------------------------------------------

fn baseline_schema_and_empty_engine_footprint() {
    let (engine, retained, peak) = measure(|| {
        let schema = basic_schema();
        Engine::new(schema)
    });

    eprintln!("[memory] empty schema+engine: {retained} bytes retained, {peak} bytes peak");

    assert!(
        retained < 64 * 1024,
        "empty schema+engine retained {retained} bytes, expected well under 64KiB"
    );

    drop(engine);
}

// ---------------------------------------------------------------------
// 2. Bytes retained per ingested sample, single dimension group — isolates
//    per-sample/bucket bookkeeping overhead from dimension growth.
// ---------------------------------------------------------------------

fn ingest_bytes_per_sample_single_group_is_bounded() {
    const N: usize = 10_000;
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap();

    let (engine, retained, peak) = measure(|| {
        let schema = basic_schema();
        let mut engine = Engine::new(schema);
        for i in 0..N {
            let t = t0 + Duration::seconds(i as i64);
            engine.ingest(t, [i as f64], ["server-a"]).unwrap();
        }
        engine
    });

    let per_sample = retained as f64 / N as f64;
    eprintln!(
        "[memory] {N} samples, 1 dimension group: {retained} bytes retained \
         ({per_sample:.1} bytes/sample), {peak} bytes peak"
    );

    assert!(
        per_sample < 512.0,
        "ingesting into a single group cost {per_sample:.1} bytes/sample, expected < 512"
    );

    drop(engine);
}

// ---------------------------------------------------------------------
// 3. Bytes retained under high dimension cardinality: N samples, each with
//    a unique "host" value -> N distinct groups. Worst case for the
//    dimension dictionary + per-group AggregateSet allocation.
// ---------------------------------------------------------------------

fn ingest_bytes_per_unique_dimension_group_is_bounded() {
    const N: usize = 5_000;
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap();

    let (engine, retained, peak) = measure(|| {
        let schema = basic_schema();
        let mut engine = Engine::new(schema);
        for i in 0..N {
            let t = t0 + Duration::seconds(i as i64);
            let host = format!("server-{i}");
            engine.ingest(t, [i as f64], [host.as_str()]).unwrap();
        }
        engine
    });

    let per_group = retained as f64 / N as f64;
    eprintln!(
        "[memory] {N} unique dimension groups: {retained} bytes retained \
         ({per_group:.1} bytes/group), {peak} bytes peak"
    );

    assert!(
        per_group < 1024.0,
        "each unique dimension group cost {per_group:.1} bytes, expected < 1024"
    );

    drop(engine);
}

// ---------------------------------------------------------------------
// 4. Memory overhead of rollup() itself, isolated from ingestion. Since
//    rollup merges already-aggregated bucket state rather than re-scanning
//    raw samples, it should be far cheaper than the ingestion phase.
// ---------------------------------------------------------------------

fn rollup_memory_overhead_is_small_relative_to_ingestion() {
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap();
    const MINUTES: i64 = 24 * 60; // one full day of minute buckets

    let (mut engine, ingest_bytes, _) = measure(|| {
        let schema = basic_schema();
        let mut engine = Engine::new(schema);
        for i in 0..MINUTES {
            engine
                .ingest(t0 + Duration::minutes(i), [i as f64], ["server-a"])
                .unwrap();
        }
        engine
    });

    let (_, rollup_bytes, rollup_peak) = measure(|| {
        engine.rollup();
    });

    eprintln!(
        "[memory] ingest {MINUTES} minute samples: {ingest_bytes} bytes; \
         rollup(): {rollup_bytes} bytes retained, {rollup_peak} bytes peak"
    );

    assert!(
        (rollup_peak as f64) < (ingest_bytes.max(1) as f64),
        "rollup() peak ({rollup_peak}) was not smaller than raw ingestion cost ({ingest_bytes})"
    );

    drop(engine);
}

// ---------------------------------------------------------------------
// 5. Repeated merging should not leak: merge many times, then confirm
//    we're back near the pre-loop baseline once temporaries are dropped.
// ---------------------------------------------------------------------

fn repeated_aggregate_set_merges_do_not_leak() {
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

    let (_, retained, _) = measure(|| {
        let mut a = schema.empty_set(MeasureId(0)).unwrap();
        a.update(&make_sample(1.0));

        for i in 0..10_000 {
            let mut b = schema.empty_set(MeasureId(0)).unwrap();
            b.update(&make_sample(i as f64));
            a.merge(&b);
            // b drops at the end of each iteration.
        }

        a.get::<Sum<f64>>().unwrap().value()
    });

    eprintln!("[memory] after 10,000 merge iterations, net retained: {retained} bytes");

    assert!(
        retained.unsigned_abs() < 8 * 1024,
        "10,000 merge iterations leaked or retained {retained} bytes, expected < 8KiB"
    );
}

// ---------------------------------------------------------------------
// 6. Bucket::merge memory cost at increasing group counts, without going
//    through Engine (isolates Bucket + AggregateSet cost).
// ---------------------------------------------------------------------

fn bucket_merge_bytes_scale_roughly_linearly_with_group_count() {
    let schema = basic_schema();
    let start = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();

    let make_bucket = |dictionaries: &mut DimensionDictionaries, groups: usize| {
        let mut bucket = Bucket::new(BucketLevel::Minute, start);
        for i in 0..groups {
            let id = dictionaries.dictionaries[0].get_or_insert(&format!("server-{i}"));
            let s = Sample::new(
                Utc::now(),
                MeasureValues::new(vec![MeasureValue::F64(i as f64)]),
                DimensionValues::new(vec![id]),
            );
            bucket.update(&s, &schema);
        }
        bucket
    };

    let mut dictionaries_small = DimensionDictionaries::new(schema.dimension_count());
    let small = make_bucket(&mut dictionaries_small, 100);
    let small_source = make_bucket(&mut dictionaries_small, 100); // same keys -> in-place merge

    let (_, small_merge_bytes, _) = measure(|| {
        let mut target = small.clone();
        target.merge(&small_source);
        target
    });

    let mut dictionaries_big = DimensionDictionaries::new(schema.dimension_count());
    let big = make_bucket(&mut dictionaries_big, 2_000);
    let big_source = make_bucket(&mut dictionaries_big, 2_000);

    let (_, big_merge_bytes, _) = measure(|| {
        let mut target = big.clone();
        target.merge(&big_source);
        target
    });

    eprintln!(
        "[memory] bucket merge, 100 matching groups: {small_merge_bytes} bytes; \
         2,000 matching groups: {big_merge_bytes} bytes"
    );

    // 20x more groups shouldn't cost drastically more than ~20-40x the
    // bytes for merging matching (in-place) groups — generous slack for
    // allocator overhead/fragmentation, just guarding against something
    // accidentally quadratic.
    assert!(
        (big_merge_bytes as f64) < (small_merge_bytes.max(1) as f64) * 100.0,
        "bucket merge cost did not scale roughly linearly: {small_merge_bytes} -> {big_merge_bytes}"
    );
}
