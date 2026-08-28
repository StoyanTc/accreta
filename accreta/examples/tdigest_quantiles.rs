//! Approximate quantiles with `TDigest`.
//!
//! Walks through registering `TDigest` alongside the exact built-ins on a "latency" measure,
//! ingesting a spread of samples across several minutes, rolling up to an hour bucket, and
//! reading back quantile estimates. Also demonstrates the one property `TDigest` does *not*
//! share with the other built-in aggregates: merges are only *approximately* associative, so
//! two different (but equally valid) merge orders over the same samples produce digests that
//! are not `==`, but whose `quantile()` answers agree within a small tolerance.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example tdigest_quantiles
//! ```

use accreta::Aggregator;
use accreta::aggregate_set::Schema;
use accreta::aggregates::{Average, Count, TDigest};
use accreta::bucket::BucketLevel;
use accreta::engine::Engine;
use accreta::measures::MeasureId;
use accreta::monoid::Monoid;
use chrono::{Duration, TimeZone, Utc};

fn main() {
    // 1. Register TDigest alongside Average on the same measure. TDigest is deliberately
    //    heavier than the exact aggregates, so it's registered only on the one measure that
    //    actually needs quantiles ("request_latency_ms") — Average stays the cheap, exact
    //    general-purpose mean for the same measure, and other measures in a real schema
    //    wouldn't get TDigest at all unless they too needed quantile queries.
    let mut builder = Schema::builder();
    builder
        .dimension("route")
        .measure("request_latency_ms")
        .with_any::<Count>()
        .with::<Average<f64>>()
        .with::<TDigest>();
    let schema = builder.build().unwrap();

    let mut engine = Engine::new(schema);

    // 2. Ingest a spread of latency samples across a few minutes, all for the same route.
    //    Nothing here is TDigest-specific — ingestion looks identical to any other measure.
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();
    let latencies_ms: &[f64] = &[
        12.0, 15.0, 11.0, 20.0, 14.0, 9.0, 100.0, 13.0, 16.0, 12.0, 18.0, 250.0, 14.0, 15.0, 11.0,
        13.0, 17.0, 12.0, 19.0, 500.0,
    ];
    for (i, &latency) in latencies_ms.iter().enumerate() {
        let t = t0 + Duration::minutes(i as i64);
        engine.ingest(t, [latency], ["/api/search"]).unwrap();
    }

    // 3. Roll up to the hour, same as any other measure — TDigest merges through the rollup
    //    hierarchy exactly like the exact aggregates do, it just doesn't guarantee an exact
    //    answer at the end.
    engine.rollup();

    let hour_start = BucketLevel::Hour.truncate(t0);
    let hour = engine.bucket(BucketLevel::Hour, hour_start).unwrap();
    let (_, sets) = hour.groups().next().unwrap();
    let latency_set = &sets[MeasureId(0).index()];

    let count = latency_set.get::<Count>().unwrap().value();
    let mean = latency_set.get::<Average<f64>>().unwrap().sum()
        / latency_set.get::<Average<f64>>().unwrap().count() as f64;
    let digest = latency_set.get::<TDigest>().unwrap();

    println!("samples ingested : {count}");
    println!("exact mean       : {mean:.1} ms");
    println!("p50 (median)     : {:.1} ms", digest.quantile(0.50));
    println!("p95              : {:.1} ms", digest.quantile(0.95));
    println!("p99              : {:.1} ms", digest.quantile(0.99));

    // The mean is dragged upward by the outliers (100, 250, 500 ms) far more than the median
    // is — a good illustration of why you'd want both an exact mean *and* quantiles on the
    // same measure rather than relying on the mean alone to characterize latency.
    assert!(
        digest.quantile(0.50) < mean,
        "median should sit below the outlier-skewed mean"
    );

    // 4. Approximate associativity: merging the same samples in a different grouping produces
    //    a digest whose *quantile answers* agree closely with the original, even though the
    //    two digests are not structurally `==`. This is the tradeoff called out in TDigest's
    //    module docs — don't rely on `==` to check "these represent the same distribution."
    let mut by_thirds = TDigest::identity();
    for chunk in latencies_ms.chunks(7) {
        let mut piece = TDigest::identity();
        for &v in chunk {
            piece.update_in_place(v);
        }
        by_thirds.merge_in_place(&piece);
    }

    let mut all_at_once = TDigest::identity();
    for &v in latencies_ms {
        all_at_once.update_in_place(v);
    }

    let p50_a = by_thirds.quantile(0.50);
    let p50_b = all_at_once.quantile(0.50);
    println!("\np50 via chunked merges : {p50_a:.2} ms\np50 via single digest  : {p50_b:.2} ms");
    assert!(
        (p50_a - p50_b).abs() < 1.0,
        "merge order shouldn't meaningfully change the quantile estimate"
    );
}
