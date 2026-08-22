//! Integration tests for `accreta` — the mergeable-state aggregation engine.
//!
//! Verified against the real `aggregate_set.rs`, `dimensions.rs`, and
//! `bucket.rs` sources. The only signatures NOT confirmed from source are
//! `Engine::new`, `Engine::ingest`, `Engine::rollup`, and `Engine::bucket`
//! (`engine.rs` wasn't provided) — those are used exactly as shown in the
//! crate's own module-level doctest, so they should already compile.
//!
//! Place this file at `tests/accreta_integration_tests.rs` in the crate root.

use chrono::{Duration, TimeZone, Utc};

use accreta::aggregate_set::Schema;
use accreta::aggregates::{Count, Sum};
use accreta::bucket::{Bucket, BucketLevel};
use accreta::dimensions::{DimensionDictionaries, DimensionValueId, DimensionValues};
use accreta::engine::Engine;
use accreta::errors::SchemaError;
use accreta::measures::{MeasureId, MeasureValue, MeasureValues};
use accreta::sample::Sample;

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// One dimension ("host"), one measure ("value"), tracking Sum<f64> + Count
/// — the same schema used in the crate's own quick-start doctest.
fn basic_schema() -> Schema {
    let mut builder = Schema::builder();
    builder
        .dimension("host")
        .measure("value")
        .with::<Sum<f64>>()
        .with_any::<Count>();
    builder.build().expect("schema should build")
}

/// Build a `Sample` for `basic_schema()`'s single dimension/measure, looking
/// up (or allocating) `dim_value`'s ID in `dictionaries`.
fn sample_for(
    dictionaries: &mut DimensionDictionaries,
    dim_value: &str,
    measure_value: f64,
) -> Sample {
    let id = dictionaries.dictionaries[0].get_or_insert(dim_value);
    Sample::new(
        Utc::now(),
        MeasureValues::new(vec![MeasureValue::F64(measure_value)]),
        DimensionValues::new(vec![id]),
    )
}

// ---------------------------------------------------------------------
// 1. Basic ingest + rollup correctness (mirrors the quick-start doctest,
//    with an extra sample).
// ---------------------------------------------------------------------

#[test]
fn ingest_then_rollup_produces_correct_hour_bucket() {
    let schema = basic_schema();
    let mut engine = Engine::new(schema);

    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 5, 0).unwrap();
    engine.ingest(t0, [12.0], ["server-a"]).unwrap();
    engine
        .ingest(t0 + Duration::minutes(1), [8.0], ["server-a"])
        .unwrap();
    engine
        .ingest(t0 + Duration::minutes(2), [5.0], ["server-a"])
        .unwrap();

    engine.rollup();

    let hour_start = BucketLevel::Hour.truncate(t0);
    let hour = engine
        .bucket(BucketLevel::Hour, hour_start)
        .expect("hour bucket should exist after rollup");

    let (_, sets) = hour.groups().next().expect("one dimension group expected");
    let value = &sets[MeasureId(0).index()];

    assert_eq!(value.get::<Sum<f64>>().unwrap().value(), 25.0);
    assert_eq!(value.get::<Count>().unwrap().value(), 3);
}

// ---------------------------------------------------------------------
// 2. Samples in different hours must not bleed into each other's bucket.
// ---------------------------------------------------------------------

#[test]
fn samples_in_different_hours_stay_isolated() {
    let schema = basic_schema();
    let mut engine = Engine::new(schema);

    let hour1 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();
    let hour2 = Utc.with_ymd_and_hms(2026, 3, 15, 11, 0, 0).unwrap();

    engine
        .ingest(hour1 + Duration::minutes(30), [10.0], ["server-a"])
        .unwrap();
    engine
        .ingest(hour2 + Duration::minutes(30), [99.0], ["server-a"])
        .unwrap();

    engine.rollup();

    let bucket1 = engine
        .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(hour1))
        .unwrap();
    let bucket2 = engine
        .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(hour2))
        .unwrap();

    let sum1 = bucket1.groups().next().unwrap().1[MeasureId(0).index()]
        .get::<Sum<f64>>()
        .unwrap()
        .value();
    let sum2 = bucket2.groups().next().unwrap().1[MeasureId(0).index()]
        .get::<Sum<f64>>()
        .unwrap()
        .value();

    assert_eq!(sum1, 10.0);
    assert_eq!(sum2, 99.0);
}

// ---------------------------------------------------------------------
// 3. Distinct dimension values stay in separate groups. Matched on the
//    aggregate values themselves rather than the group key, since
//    `DimensionKey` stores integer `DimensionValueId`s, not the original
//    strings — no dictionary reverse-lookup is exposed through Engine.
// ---------------------------------------------------------------------

#[test]
fn distinct_dimension_values_stay_in_separate_groups() {
    let schema = basic_schema();
    let mut engine = Engine::new(schema);

    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();
    engine.ingest(t0, [10.0], ["server-a"]).unwrap();
    engine
        .ingest(t0 + Duration::minutes(1), [20.0], ["server-b"])
        .unwrap();
    engine
        .ingest(t0 + Duration::minutes(2), [5.0], ["server-a"])
        .unwrap();

    engine.rollup();

    let hour = engine
        .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(t0))
        .unwrap();

    let groups: Vec<_> = hour.groups().collect();
    assert_eq!(groups.len(), 2, "expected two distinct dimension groups");

    let mut observed: Vec<(f64, u64)> = groups
        .iter()
        .map(|(_, sets)| {
            let set = &sets[MeasureId(0).index()];
            (
                set.get::<Sum<f64>>().unwrap().value(),
                set.get::<Count>().unwrap().value(),
            )
        })
        .collect();
    observed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    assert_eq!(observed, vec![(15.0, 2), (20.0, 1)]);
}

// ---------------------------------------------------------------------
// 4. Querying a bucket with no ingested data returns None.
// ---------------------------------------------------------------------

#[test]
fn querying_an_empty_bucket_returns_none() {
    let schema = basic_schema();
    let mut engine = Engine::new(schema);

    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();
    engine.ingest(t0, [1.0], ["server-a"]).unwrap();
    engine.rollup();

    let empty_hour = t0 + Duration::hours(5);
    let result = engine.bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(empty_hour));

    assert!(result.is_none());
}

// ---------------------------------------------------------------------
// 5. `Day` rolls up directly into BOTH `Week` and `Month` (per
//    `BucketLevel::rollup_targets`, `Day => [Week, Month]`) — this is not
//    a simple chain, so both must independently reflect the same day's
//    data even though `Week` itself never rolls up any further.
// ---------------------------------------------------------------------

#[test]
fn day_rolls_up_directly_into_both_week_and_month() {
    let schema = basic_schema();
    let mut engine = Engine::new(schema);

    // 2026-03-02 is a Monday, so week-start and month-start diverge.
    let day = Utc.with_ymd_and_hms(2026, 3, 2, 0, 0, 0).unwrap();
    engine
        .ingest(day + Duration::hours(3), [4.0], ["server-a"])
        .unwrap();
    engine
        .ingest(day + Duration::hours(10), [6.0], ["server-a"])
        .unwrap();

    engine.rollup();

    let week = engine
        .bucket(BucketLevel::Week, BucketLevel::Week.truncate(day))
        .expect("week bucket should exist");
    let month = engine
        .bucket(BucketLevel::Month, BucketLevel::Month.truncate(day))
        .expect("month bucket should exist");

    for bucket in [week, month] {
        let (_, sets) = bucket.groups().next().unwrap();
        let value = &sets[MeasureId(0).index()];
        assert_eq!(value.get::<Sum<f64>>().unwrap().value(), 10.0);
        assert_eq!(value.get::<Count>().unwrap().value(), 2);
    }
}

// ---------------------------------------------------------------------
// 6. `Year` is fed via `Month` (Month => [Year]), not via `Week`
//    (Week => []) — confirms the fan-out in test 5 doesn't accidentally
//    stall the hierarchy above `Week`.
// ---------------------------------------------------------------------

#[test]
fn year_bucket_is_populated_via_month_not_week() {
    let schema = basic_schema();
    let mut engine = Engine::new(schema);

    let day = Utc.with_ymd_and_hms(2026, 6, 10, 0, 0, 0).unwrap();
    engine
        .ingest(day + Duration::hours(2), [7.0], ["server-a"])
        .unwrap();
    engine.rollup();

    let year = engine
        .bucket(BucketLevel::Year, BucketLevel::Year.truncate(day))
        .expect("year bucket should exist");

    let (_, sets) = year.groups().next().unwrap();
    let value = &sets[MeasureId(0).index()];
    assert_eq!(value.get::<Sum<f64>>().unwrap().value(), 7.0);
    assert_eq!(value.get::<Count>().unwrap().value(), 1);
}

// ---------------------------------------------------------------------
// 7. December/January rollover: month and year buckets for a December
//    sample must land in 2026, and nothing should leak into January 2027.
// ---------------------------------------------------------------------

#[test]
fn month_and_year_rollup_handle_december_rollover_correctly() {
    let schema = basic_schema();
    let mut engine = Engine::new(schema);

    let dec_day = Utc.with_ymd_and_hms(2026, 12, 20, 0, 0, 0).unwrap();
    engine
        .ingest(dec_day + Duration::hours(5), [42.0], ["server-a"])
        .unwrap();
    engine.rollup();

    let month = engine
        .bucket(BucketLevel::Month, BucketLevel::Month.truncate(dec_day))
        .expect("december month bucket should exist");
    let year = engine
        .bucket(BucketLevel::Year, BucketLevel::Year.truncate(dec_day))
        .expect("2026 year bucket should exist");

    for bucket in [month, year] {
        let (_, sets) = bucket.groups().next().unwrap();
        let value = &sets[MeasureId(0).index()];
        assert_eq!(value.get::<Sum<f64>>().unwrap().value(), 42.0);
    }

    let jan_2027 = Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap();
    assert!(engine.bucket(BucketLevel::Month, jan_2027).is_none());
    assert!(engine.bucket(BucketLevel::Year, jan_2027).is_none());
}

// ---------------------------------------------------------------------
// 8. Order-independence: ingesting the same samples in a different order
//    must produce an identical merged result.
// ---------------------------------------------------------------------

#[test]
fn ingest_order_does_not_affect_merged_result() {
    let t0 = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();
    let samples = [(0, 3.0), (10, 7.0), (20, 2.0), (30, 9.0), (40, 1.0)];

    let sum_of = |order: &[(i64, f64)]| -> (f64, u64) {
        let schema = basic_schema();
        let mut engine = Engine::new(schema);
        for &(offset, value) in order {
            engine
                .ingest(t0 + Duration::minutes(offset), [value], ["server-a"])
                .unwrap();
        }
        engine.rollup();
        let hour = engine
            .bucket(BucketLevel::Hour, BucketLevel::Hour.truncate(t0))
            .unwrap();
        let (_, sets) = hour.groups().next().unwrap();
        let value = &sets[MeasureId(0).index()];
        (
            value.get::<Sum<f64>>().unwrap().value(),
            value.get::<Count>().unwrap().value(),
        )
    };

    let forward = sum_of(&samples);
    let mut reversed = samples;
    reversed.reverse();
    let backward = sum_of(&reversed);

    assert_eq!(forward, backward);
}

// ---------------------------------------------------------------------
// 9. Schema validation: confirmed against source — `build()` returns
//    `SchemaError::NoDimensions` when no dimension was registered, even
//    if a measure was, and `SchemaError::NoMeasures` symmetrically.
// ---------------------------------------------------------------------

#[test]
fn schema_without_a_dimension_fails_with_no_dimensions() {
    let mut builder = Schema::builder();
    builder.measure::<f64>("value").with::<Sum<f64>>();
    let result = builder.build();
    assert!(matches!(result, Err(SchemaError::NoDimensions)));
}

#[test]
fn schema_without_a_measure_fails_with_no_measures() {
    let mut builder = Schema::builder();
    builder.dimension("host");
    let result = builder.build();
    assert!(matches!(result, Err(SchemaError::NoMeasures)));
}

// ---------------------------------------------------------------------
// 10. `AggregateSet::merge` is commutative and associative for Sum and
//     Count — the property the whole rollup hierarchy depends on. Built
//     entirely from the confirmed `AggregateSet`/`Sample` API, bypassing
//     `Engine` and `Bucket` altogether.
// ---------------------------------------------------------------------

#[test]
fn aggregate_set_merge_is_commutative_and_associative() {
    let schema = basic_schema();
    let mut dictionaries = DimensionDictionaries::new(schema.dimension_count());

    let mut a = schema.empty_set(MeasureId(0)).unwrap();
    a.update(&sample_for(&mut dictionaries, "server-a", 3.0));

    let mut b = schema.empty_set(MeasureId(0)).unwrap();
    b.update(&sample_for(&mut dictionaries, "server-a", 4.0));
    b.update(&sample_for(&mut dictionaries, "server-a", 5.0));

    let mut c = schema.empty_set(MeasureId(0)).unwrap();
    c.update(&sample_for(&mut dictionaries, "server-a", 6.0));

    // Associativity: (a merge b) merge c == a merge (b merge c).
    let mut left = a.clone();
    left.merge(&b);
    left.merge(&c);

    let mut bc = b.clone();
    bc.merge(&c);
    let mut right = a.clone();
    right.merge(&bc);

    assert_eq!(
        left.get::<Sum<f64>>().unwrap().value(),
        right.get::<Sum<f64>>().unwrap().value()
    );
    assert_eq!(
        left.get::<Count>().unwrap().value(),
        right.get::<Count>().unwrap().value()
    );
    assert_eq!(left.get::<Sum<f64>>().unwrap().value(), 18.0);
    assert_eq!(left.get::<Count>().unwrap().value(), 4);

    // Commutativity: a merge b == b merge a.
    let mut ab = a.clone();
    ab.merge(&b);
    let mut ba = b.clone();
    ba.merge(&a);

    assert_eq!(
        ab.get::<Sum<f64>>().unwrap().value(),
        ba.get::<Sum<f64>>().unwrap().value()
    );
    assert_eq!(
        ab.get::<Count>().unwrap().value(),
        ba.get::<Count>().unwrap().value()
    );
}

// ---------------------------------------------------------------------
// 11. `Bucket::merge` combines matching dimension groups and preserves
//     groups that only exist on one side — the mechanism that makes the
//     minute -> hour -> ... hierarchy mergeable at all.
// ---------------------------------------------------------------------

#[test]
fn bucket_merge_combines_matching_groups_and_keeps_disjoint_ones() {
    let schema = basic_schema();
    let mut dictionaries = DimensionDictionaries::new(schema.dimension_count());
    let a_id: DimensionValueId = dictionaries.dictionaries[0].get_or_insert("server-a");
    let b_id: DimensionValueId = dictionaries.dictionaries[0].get_or_insert("server-b");

    let make_sample = |dim_id: DimensionValueId, value: f64| {
        Sample::new(
            Utc::now(),
            MeasureValues::new(vec![MeasureValue::F64(value)]),
            DimensionValues::new(vec![dim_id]),
        )
    };

    let start = Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap();

    let mut bucket1 = Bucket::new(BucketLevel::Minute, start);
    bucket1.update(&make_sample(a_id, 1.0), &schema);
    bucket1.update(&make_sample(b_id, 100.0), &schema);

    let mut bucket2 = Bucket::new(BucketLevel::Minute, start);
    bucket2.update(&make_sample(a_id, 2.0), &schema);
    // bucket2 has no "server-b" sample at all.

    bucket1.merge(&bucket2);

    assert_eq!(bucket1.group_count(), 2);

    let a_key = DimensionValues::new(vec![a_id]).full_key();
    let b_key = DimensionValues::new(vec![b_id]).full_key();

    let a_group = bucket1.group(&a_key).expect("server-a group should exist");
    let a_value = &a_group[MeasureId(0).index()];
    assert_eq!(a_value.get::<Sum<f64>>().unwrap().value(), 3.0);
    assert_eq!(a_value.get::<Count>().unwrap().value(), 2);

    let b_group = bucket1.group(&b_key).expect("server-b group should exist");
    let b_value = &b_group[MeasureId(0).index()];
    assert_eq!(b_value.get::<Sum<f64>>().unwrap().value(), 100.0);
    assert_eq!(b_value.get::<Count>().unwrap().value(), 1);
}
