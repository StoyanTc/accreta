//! Integration tests for `monoid` — edge cases around ingestion, rollup, and merge semantics.
//!
//! NOTE: These are written against the public surface shown in `lib.rs`'s doc comments
//! (`Schema::builder`, `Engine::new/ingest/rollup/bucket`, `BucketLevel::truncate`,
//! `aggregates::{Sum, Count, Min, Max, Average}`, `MeasureId`). A few calls (error variants,
//! `Retention` construction, `DimensionMask`) are inferred from the module map and may need
//! small signature adjustments once wired against the real crate — the intent of each test
//! should carry over regardless.

use accreta::aggregate_set::Schema;
use accreta::aggregates::{Average, Count, Max, Min, Sum};
use accreta::bucket::BucketLevel;
use accreta::engine::Engine;
use accreta::measures::MeasureId;
use chrono::{Duration, TimeZone, Utc};
// ASSUMPTION: `SchemaError` lives in the `errors` module per the lib.rs module
// map. Adjust this path if it's actually re-exported from `aggregate_set`.
use accreta::errors::SchemaError;

fn schema_single_measure() -> Schema {
    let mut builder = Schema::builder();
    builder
        .dimension("host")
        .measure("value")
        .with::<Sum<f64>>()
        .with_any::<Count>()
        .with::<Min<f64>>()
        .with::<Max<f64>>();
    builder.build().unwrap()
}

// ---------------------------------------------------------------------------
// Empty / not-yet-populated state
// ---------------------------------------------------------------------------

#[test]
fn query_before_any_ingest_returns_none() {
    let engine = Engine::new(schema_single_measure());
    let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let minute = BucketLevel::Minute.truncate(t);
    assert!(engine.bucket(BucketLevel::Minute, minute).is_none());
}

#[test]
fn rollup_with_no_data_is_a_no_op() {
    let mut engine = Engine::new(schema_single_measure());
    // Should not panic even though nothing has been ingested yet.
    engine.rollup();
    let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    assert!(
        engine
            .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(t))
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// Bucket boundary handling
// ---------------------------------------------------------------------------

#[test]
fn sample_exactly_on_hour_boundary_belongs_to_new_hour() {
    let mut engine = Engine::new(schema_single_measure());
    let boundary = Utc.with_ymd_and_hms(2026, 3, 15, 11, 0, 0).unwrap();
    let just_before = boundary - Duration::seconds(1);

    engine.ingest(just_before, [1.0], ["server-a"]).unwrap();
    engine.ingest(boundary, [2.0], ["server-a"]).unwrap();
    engine.rollup();

    let prev_hour = engine
        .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(just_before))
        .unwrap();
    let new_hour = engine
        .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(boundary))
        .unwrap();

    let (_, prev_sets) = prev_hour.groups().next().unwrap();
    let (_, new_sets) = new_hour.groups().next().unwrap();

    assert_eq!(
        prev_sets[MeasureId(0).index()]
            .get::<Sum<f64>>()
            .unwrap()
            .value(),
        1.0
    );
    assert_eq!(
        new_sets[MeasureId(0).index()]
            .get::<Sum<f64>>()
            .unwrap()
            .value(),
        2.0
    );
}

// ---------------------------------------------------------------------------
// Out-of-order ingestion
// ---------------------------------------------------------------------------

#[test]
fn out_of_order_samples_still_aggregate_correctly() {
    let mut engine = Engine::new(schema_single_measure());
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();

    // Ingest in reverse chronological order within the same minute-level rollup window.
    engine
        .ingest(t0 + Duration::seconds(40), [3.0], ["server-a"])
        .unwrap();
    engine
        .ingest(t0 + Duration::seconds(10), [1.0], ["server-a"])
        .unwrap();
    engine
        .ingest(t0 + Duration::seconds(25), [2.0], ["server-a"])
        .unwrap();
    engine.rollup();

    let minute = engine
        .bucket(BucketLevel::Minute, BucketLevel::Minute.truncate(t0))
        .unwrap();
    let (_, sets) = minute.groups().next().unwrap();
    let value = &sets[MeasureId(0).index()];

    assert_eq!(value.get::<Sum<f64>>().unwrap().value(), 6.0);
    assert_eq!(value.get::<Count>().unwrap().value(), 3);
    assert_eq!(value.get::<Min<f64>>().unwrap().value(), Some(1.0));
    assert_eq!(value.get::<Max<f64>>().unwrap().value(), Some(3.0));
}

// ---------------------------------------------------------------------------
// Multi-dimension grouping
// ---------------------------------------------------------------------------

#[test]
fn disjoint_dimension_values_stay_isolated_until_merged() {
    let mut engine = Engine::new(schema_single_measure());
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();

    engine.ingest(t0, [10.0], ["server-a"]).unwrap();
    engine.ingest(t0, [20.0], ["server-b"]).unwrap();
    engine.rollup();

    let hour = engine
        .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(t0))
        .unwrap();

    // Two distinct dimension groups, each with independent state.
    assert_eq!(hour.groups().count(), 2);

    for (key, sets) in hour.groups() {
        let sum = sets[MeasureId(0).index()]
            .get::<Sum<f64>>()
            .unwrap()
            .value();
        match key.values() {
            [0] => assert_eq!(sum, 10.0),
            [1u32] => assert_eq!(sum, 20.0),
            other => panic!("unexpected dimension value: {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Rollup correctness across multiple levels
// ---------------------------------------------------------------------------

#[test]
fn rollup_is_pure_merge_not_a_rescan() {
    // Three separate minute buckets, same hour. After rollup, the hour bucket's
    // aggregate must equal the merge of the three minute states — verified by
    // comparing against manually merged expectations rather than re-deriving from
    // raw samples (which the engine is documented to never do).
    let mut engine = Engine::new(schema_single_measure());
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();

    let values = [5.0, 7.0, 2.0];
    for (i, v) in values.iter().enumerate() {
        engine
            .ingest(t0 + Duration::minutes(i as i64), [*v], ["server-a"])
            .unwrap();
    }
    engine.rollup();

    let hour = engine
        .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(t0))
        .unwrap();
    let (_, sets) = hour.groups().next().unwrap();
    let value = &sets[MeasureId(0).index()];

    assert_eq!(value.get::<Sum<f64>>().unwrap().value(), 14.0);
    assert_eq!(value.get::<Count>().unwrap().value(), 3);
    assert_eq!(value.get::<Min<f64>>().unwrap().value(), Some(2.0));
    assert_eq!(value.get::<Max<f64>>().unwrap().value(), Some(7.0));
}

#[test]
fn calling_rollup_twice_does_not_double_count() {
    let mut engine = Engine::new(schema_single_measure());
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();

    engine.ingest(t0, [4.0], ["server-a"]).unwrap();
    engine.rollup();
    engine.rollup(); // idempotent: no new raw samples, must not re-merge into itself

    let hour = engine
        .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(t0))
        .unwrap();
    let (_, sets) = hour.groups().next().unwrap();
    assert_eq!(
        sets[MeasureId(0).index()]
            .get::<Sum<f64>>()
            .unwrap()
            .value(),
        4.0
    );
    assert_eq!(
        sets[MeasureId(0).index()].get::<Count>().unwrap().value(),
        1
    );
}

// ---------------------------------------------------------------------------
// Derived aggregate (Average) built from Sum + Count
// ---------------------------------------------------------------------------

#[test]
fn average_reflects_merged_sum_and_count_not_average_of_averages() {
    let mut builder = Schema::builder();
    builder
        .dimension("host")
        .measure("value")
        .with::<Sum<f64>>()
        .with_any::<Count>()
        .with::<Average<f64>>();
    let schema = builder.build().unwrap();
    let mut engine = Engine::new(schema);

    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();
    // Minute 0: single sample of 10 (avg 10). Minute 1: three samples averaging 1 each.
    engine.ingest(t0, [10.0], ["server-a"]).unwrap();
    engine
        .ingest(t0 + Duration::minutes(1), [1.0], ["server-a"])
        .unwrap();
    engine
        .ingest(
            t0 + Duration::minutes(1) + Duration::seconds(1),
            [1.0],
            ["server-a"],
        )
        .unwrap();
    engine
        .ingest(
            t0 + Duration::minutes(1) + Duration::seconds(2),
            [1.0],
            ["server-a"],
        )
        .unwrap();
    engine.rollup();

    let hour = engine
        .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(t0))
        .unwrap();
    let (_, sets) = hour.groups().next().unwrap();
    let avg = sets[0].get::<Average<f64>>().unwrap();
    let avg_val = avg.sum() / avg.count() as f64;

    // Correct: (10 + 1 + 1 + 1) / 4 = 3.25
    // Wrong (naive average-of-averages): (10 + 1) / 2 = 5.5
    assert_eq!(avg_val, 3.25);
}

// ---------------------------------------------------------------------------
// Schema validation edge cases
// ---------------------------------------------------------------------------

#[test]
fn schema_with_no_dimensions_fails_to_build() {
    let mut builder = Schema::builder();
    // `.measure::<f64>(..)` needs an explicit type argument (or a chained
    // `.with::<A>()` to unify against) — `SchemaBuilder::measure<T>` can't
    // infer `T` from nothing.
    builder.measure::<f64>("value").with::<Sum<f64>>();
    assert!(matches!(builder.build(), Err(SchemaError::NoDimensions)));
}

#[test]
fn schema_with_no_measures_fails_to_build() {
    let mut builder = Schema::builder();
    builder.dimension("host");
    assert!(matches!(builder.build(), Err(SchemaError::NoMeasures)));
}

#[test]
fn measure_with_no_aggregates_builds_but_has_no_lookups() {
    // `SchemaBuilder::build` only validates `NoDimensions` / `NoMeasures` — it
    // does *not* reject a measure that never had `.with::<..>()` called on it.
    // So this is not an error case: confirm it builds, and that the resulting
    // (empty) AggregateSet correctly reports no aggregate under that name
    // rather than panicking on `get::<T>()`.
    let mut builder = Schema::builder();
    builder.dimension("host").measure::<f64>("value"); // no `.with::<..>()` calls
    let schema = builder
        .build()
        .expect("a measure with zero registered aggregates still builds");

    let set = schema.empty_set(MeasureId(0)).unwrap();
    assert!(set.get::<Sum<f64>>().is_none());
}

#[test]
fn ingest_arity_mismatch_is_rejected_not_panicking() {
    let mut engine = Engine::new(schema_single_measure());
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();
    // Schema declares exactly one dimension ("host"); supply two instead.
    // (Deliberately a concrete over-supply rather than `[]` — an empty array
    // literal's element type can be ambiguous if `ingest`'s dimensions
    // parameter is behind an unconstrained generic bound like
    // `impl IntoIterator<Item = &str>`, whereas a populated mismatched array
    // is unambiguous under any such signature.)
    let result = engine.ingest(t0, [1.0], ["server-a", "unexpected-extra"]);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Numeric edge cases within a single aggregate
// ---------------------------------------------------------------------------

#[test]
fn min_max_handle_negative_and_zero_values() {
    let mut engine = Engine::new(schema_single_measure());
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();

    for v in [-5.0, 0.0, 5.0, -100.0, 42.0] {
        engine.ingest(t0, [v], ["server-a"]).unwrap();
    }
    engine.rollup();

    let minute = engine
        .bucket(BucketLevel::Minute, BucketLevel::Minute.truncate(t0))
        .unwrap();
    let (_, sets) = minute.groups().next().unwrap();
    let value = &sets[MeasureId(0).index()];

    assert_eq!(value.get::<Min<f64>>().unwrap().value(), Some(-100.0));
    assert_eq!(value.get::<Max<f64>>().unwrap().value(), Some(42.0));
}

#[test]
fn single_sample_min_equals_max_equals_sum() {
    let mut engine = Engine::new(schema_single_measure());
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();
    engine.ingest(t0, [7.5], ["server-a"]).unwrap();
    engine.rollup();

    let minute = engine
        .bucket(BucketLevel::Minute, BucketLevel::Minute.truncate(t0))
        .unwrap();
    let (_, sets) = minute.groups().next().unwrap();
    let value = &sets[MeasureId(0).index()];

    assert_eq!(value.get::<Min<f64>>().unwrap().value(), Some(7.5));
    assert_eq!(value.get::<Max<f64>>().unwrap().value(), Some(7.5));
    assert_eq!(value.get::<Sum<f64>>().unwrap().value(), 7.5);
    assert_eq!(value.get::<Count>().unwrap().value(), 1);
}

// ---------------------------------------------------------------------------
// Cross-year / large time-gap rollups
// ---------------------------------------------------------------------------

#[test]
fn samples_years_apart_land_in_disjoint_year_buckets() {
    let mut engine = Engine::new(schema_single_measure());
    let early = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let late = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

    engine.ingest(early, [1.0], ["server-a"]).unwrap();
    engine.ingest(late, [2.0], ["server-a"]).unwrap();
    engine.rollup();

    let early_year = engine
        .bucket(BucketLevel::Year, BucketLevel::Year.truncate(early))
        .unwrap();
    let late_year = engine
        .bucket(BucketLevel::Year, BucketLevel::Year.truncate(late))
        .unwrap();

    let (_, early_sets) = early_year.groups().next().unwrap();
    let (_, late_sets) = late_year.groups().next().unwrap();

    assert_eq!(
        early_sets[MeasureId(0).index()]
            .get::<Sum<f64>>()
            .unwrap()
            .value(),
        1.0
    );
    assert_eq!(
        late_sets[MeasureId(0).index()]
            .get::<Sum<f64>>()
            .unwrap()
            .value(),
        2.0
    );
}
