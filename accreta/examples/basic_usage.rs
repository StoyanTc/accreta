//! Basic usage: ingest a stream of samples spanning a couple of hours, roll them up through
//! every bucket level, and read back aggregates at different granularities.
//!
//! Run with:
//! ```text
//! cargo run --example basic_usage
//! ```

use accreta::aggregate_set::Schema;
use accreta::aggregates::{Average, Count, Max, Min, Sum};
use accreta::bucket::BucketLevel;
use accreta::engine::Engine;
use accreta::measures::MeasureId;
use chrono::{Duration, TimeZone, Utc};

fn main() {
    // 1. Describe which aggregates every bucket should track. Adding a new one later (e.g.
    //    Variance) is just another `.with::<T>()` call — nothing else in this example
    //    would need to change. Each aggregate is registered under its own type name
    //    (`T::NAME`), so reading it back later is done by type via `AggregateSet::get::<T>()`
    //    rather than a string key — there's no `get::<Sum>("sum")` call to typo or mismatch.
    let mut builder = Schema::builder();
    builder
        .dimension("browser")
        .measure("visits")
        .with::<Sum<f64>>()
        .with_any::<Count>()
        .with::<Min<f64>>()
        .with::<Max<f64>>()
        .with::<Average<f64>>();
    let schema = builder.build().unwrap();

    let mut engine = Engine::new(schema.clone());

    // 2. Ingest some raw samples. Every sample only ever touches a minute-level bucket.
    let start = Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap();
    let readings = [
        (0, 21.5, "Firefox"),
        (1, 22.0, "Firefox"),
        (3, 19.8, "Firefox"),
        (15, 23.1, "Firefox"),
        (47, 20.4, "Firefox"),
        (61, 25.0, "Firefox"), // rolls into the next hour
        (62, 24.7, "Firefox"),
        (125, 18.9, "Firefox"), // rolls into a third hour
    ];
    for (minute_offset, value, dim_val) in readings {
        _ = engine.ingest(
            start + Duration::minutes(minute_offset),
            vec![value],
            vec![dim_val],
        );
    }
    println!(
        "ingested {} raw samples into {} minute buckets",
        readings.len(),
        engine.bucket_count(BucketLevel::Minute)
    );

    // 3. Roll everything up. This never re-reads a Sample — only merges bucket states upward
    //    through Hour -> Day -> Week -> Month -> Year.
    engine.rollup();

    // 4. Read aggregates back at whatever granularity is useful.
    println!("\nPer-hour breakdown:");
    for bucket in engine.buckets(BucketLevel::Hour) {
        for group in bucket.groups() {
            let dim_key = group.0;
            let aggs = group.1;
            for agg in aggs {
                let avg = agg.get::<Average<f64>>().unwrap();
                println!(
                    "  [{:?} {} .. {}) sum={:>6.2} count={:<2} min={:>5.2} max={:>5.2} avg={:>5.2}",
                    dim_key.values(),
                    bucket.start().format("%H:%M"),
                    bucket.end().format("%H:%M"),
                    agg.get::<Sum<f64>>().unwrap().value(),
                    agg.get::<Count>().unwrap().value(),
                    agg.get::<Min<f64>>().unwrap().value().unwrap(),
                    agg.get::<Max<f64>>().unwrap().value().unwrap(),
                    avg.sum() / avg.count() as f64
                );
            }
        }
    }

    println!("\nWhole-day total (rolled all the way up):");
    let day_start = BucketLevel::Day.truncate(start);
    let day = engine.bucket(BucketLevel::Day, day_start).unwrap();
    let aggs = day.groups().next().unwrap().1;
    for agg in aggs {
        let avg = agg.get::<Average<f64>>().unwrap();
        println!(
            "  count={} sum={:.2} average={:.2}",
            agg.get::<Count>().unwrap().value(),
            agg.get::<Sum<f64>>().unwrap().value(),
            avg.sum() / avg.count() as f64,
        );
    }

    // 5. Ad-hoc range queries merge whichever buckets already exist at a given level, without
    //    storing anything new — handy for "give me the last N hours" style queries.
    println!("\nAd-hoc query for the first two hours only:");
    let range = engine
        .query_range(
            BucketLevel::Hour,
            start,
            start + Duration::hours(2),
            MeasureId(0),
        )
        .unwrap();
    println!(
        "  count={} sum={:.2}",
        range.get::<Count>().unwrap().value(),
        range.get::<Sum<f64>>().unwrap().value(),
    );

    // 6. Retention: an Engine keeps every bucket forever by default, which is fine for a bounded
    //    batch job like this one but not for long-running ingestion. `Retention` bounds memory
    //    at whichever levels you configure; `prune` is the explicit, opt-in step that actually
    //    discards old buckets (rollup itself never deletes anything).
    println!("\nRetention: keeping only the last hour of minute-level detail");
    let schema_for_retention = {
        let mut b = Schema::builder();
        b.dimension("browser")
            .measure("visits")
            .with::<Sum<f64>>()
            .with_any::<Count>();
        b.build().unwrap()
    };
    let policy = accreta::Retention::new().keep(BucketLevel::Minute, Duration::hours(1));
    let mut bounded_engine = Engine::with_retention(schema_for_retention, policy);
    for (minute_offset, value, dim_val) in readings {
        _ = bounded_engine.ingest(
            start + Duration::minutes(minute_offset),
            vec![value],
            vec![dim_val],
        );
    }
    println!(
        "  before prune: {} minute buckets",
        bounded_engine.bucket_count(BucketLevel::Minute)
    );
    bounded_engine.prune();
    println!(
        "  after prune:  {} minute buckets (older than 1h before the newest sample were dropped)",
        bounded_engine.bucket_count(BucketLevel::Minute)
    );
}
