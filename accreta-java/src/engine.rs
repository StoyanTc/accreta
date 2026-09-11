//! JNI natives backing `com.accreta.Engine`.

use chrono::{TimeZone, Utc};
use jni::JNIEnv;
use jni::objects::{JClass, JObjectArray, JString};
use jni::sys::{jdouble, jint, jlong, jlongArray};

use accreta::aggregate_set::Schema;
use accreta::bucket::BucketLevel;
use accreta::engine::Engine;
use accreta::measures::MeasureId;
use accreta::retention::Retention;

use crate::error::throw_ingest_error;
use crate::handles::{borrow, borrow_mut, drop_handle, into_handle};

/// Maps the ordinal of `com.accreta.BucketLevel` (declared in that exact order) onto
/// `accreta::bucket::BucketLevel`. Keep the two enums in lockstep — see the Java file's
/// doc-comment for the matching contract.
pub(crate) fn bucket_level_from_ordinal(ordinal: jint) -> BucketLevel {
    match ordinal {
        0 => BucketLevel::Minute,
        1 => BucketLevel::Hour,
        2 => BucketLevel::Day,
        3 => BucketLevel::Week,
        4 => BucketLevel::Month,
        5 => BucketLevel::Year,
        other => panic!("unknown BucketLevel ordinal {other} — Java/Rust enums are out of sync"),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Engine_nativeNew(_env: JNIEnv, _class: JClass, schema_handle: jlong) -> jlong {
    // Schema is cheap to clone (Arc-backed) — mirrors how accreta-node hands the same Schema to
    // more than one Engine without transferring ownership of the Java-side Schema object.
    let schema: &Schema = unsafe { borrow(schema_handle) };
    into_handle(Engine::new(schema.clone()))
}

/// Mirrors `Engine::with_retention`. `retention` is `Copy`, so it's read by value and the
/// caller's `Retention` handle stays valid (and independently closeable) afterward.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Engine_nativeNewWithRetention(
    _env: JNIEnv,
    _class: JClass,
    schema_handle: jlong,
    retention_handle: jlong,
) -> jlong {
    let schema: &Schema = unsafe { borrow(schema_handle) };
    let retention: &Retention = unsafe { borrow(retention_handle) };
    into_handle(Engine::with_retention(schema.clone(), *retention))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Engine_nativeDrop(_env: JNIEnv, _class: JClass, handle: jlong) {
    unsafe { drop_handle::<Engine>(handle) };
}

/// Ingest one sample. `measures` are the schema's f64 measure values in registration order;
/// `dimensions` are the schema's dimension string values in registration order — same argument
/// shape as `Engine::ingest`'s `measures`/`dimensions` iterables in Rust.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Engine_nativeIngest<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    epoch_millis: jlong,
    measures: jni::objects::JDoubleArray<'local>,
    dimensions: JObjectArray<'local>,
) {
    let timestamp = match Utc.timestamp_millis_opt(epoch_millis).single() {
        Some(t) => t,
        None => {
            let _ = env.throw_new("com/accreta/IngestException", format!("invalid timestamp: {epoch_millis} ms since epoch"));
            return;
        }
    };

    let measure_len = env.get_array_length(&measures).expect("measures array") as usize;
    let mut measure_values = vec![0.0f64; measure_len];
    env.get_double_array_region(&measures, 0, &mut measure_values)
        .expect("reading measures array");

    let dim_len = env.get_array_length(&dimensions).expect("dimensions array") as usize;
    let mut dim_values: Vec<String> = Vec::with_capacity(dim_len);
    for i in 0..dim_len {
        let element = env
            .get_object_array_element(&dimensions, i as jint)
            .expect("reading dimensions array element");
        let jstr = JString::from(element);
        let s: String = env.get_string(&jstr).expect("dimension value is not valid UTF-8").into();
        dim_values.push(s);
    }

    let engine: &mut Engine = unsafe { borrow_mut(handle) };
    if let Err(err) = engine.ingest(timestamp, measure_values, dim_values) {
        throw_ingest_error(&mut env, err, ());
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Engine_nativeRollup(_env: JNIEnv, _class: JClass, handle: jlong) {
    let engine: &mut Engine = unsafe { borrow_mut(handle) };
    engine.rollup();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Engine_nativePrune(_env: JNIEnv, _class: JClass, handle: jlong) {
    let engine: &mut Engine = unsafe { borrow_mut(handle) };
    engine.prune();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Engine_nativeBucketCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    level_ordinal: jint,
) -> jlong {
    let engine: &Engine = unsafe { borrow(handle) };
    engine.bucket_count(bucket_level_from_ordinal(level_ordinal)) as jlong
}

/// Merges every matching bucket in `[range_start, range_end)` at `level` into one total
/// `AggregateSet` and returns a handle to it — mirrors `Engine::query_range`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Engine_nativeQueryRange(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    level_ordinal: jint,
    range_start_millis: jlong,
    range_end_millis: jlong,
    measure_id: jint,
) -> jlong {
    let level = bucket_level_from_ordinal(level_ordinal);
    let range_start = Utc.timestamp_millis_opt(range_start_millis).single().expect("valid range_start");
    let range_end = Utc.timestamp_millis_opt(range_end_millis).single().expect("valid range_end");

    let engine: &Engine = unsafe { borrow(handle) };
    match engine.query_range(level, range_start, range_end, MeasureId(measure_id as u8)) {
        Ok(set) => into_handle(set),
        Err(err) => crate::error::throw_schema_error(&mut env, err, 0),
    }
}

// Silence the unused-import warning on `jlongArray` / `jdouble` until grouped queries
// (`query_range_grouped`) are wired up as a follow-up — kept here as a marker of what's next.
#[allow(unused)]
fn _reserved_for_grouped_queries(_: jlongArray, _: jdouble) {}
