//! JNI natives backing `com.accreta.AggregateSet`.
//!
//! `AggregateSet::get::<T>()` is a Rust-generic lookup keyed by type; JNI has no equivalent, so
//! it's exposed as one typed getter per built-in aggregate instead (`getSum`, `getCount`, ...).
//! Each returns a sentinel plus a `hasX`/`out-param "found"` story where the aggregate wasn't
//! registered on this measure — see each method's doc below for its exact contract.

use jni::objects::JClass;
use jni::sys::{jboolean, jdouble, jlong, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use accreta::aggregate_set::AggregateSet;
use accreta::aggregates::{Average, Count, Max, Min, Sum, TDigest};

use crate::handles::{borrow, drop_handle};

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_AggregateSet_nativeDrop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    unsafe { drop_handle::<AggregateSet>(handle) };
}

/// Sum<f64>. Returns `0.0` (matching the aggregate's own identity value) if `Sum<f64>` wasn't
/// registered on this measure — callers that need to distinguish "zero" from "not registered"
/// should check the schema up front rather than relying on this return value.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_AggregateSet_nativeGetSum(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jdouble {
    let set: &AggregateSet = unsafe { borrow(handle) };
    set.get::<Sum<f64>>().map(|s| s.value()).unwrap_or(0.0)
}

/// Count. Returns `0` if `Count` wasn't registered on this measure.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_AggregateSet_nativeGetCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let set: &AggregateSet = unsafe { borrow(handle) };
    set.get::<Count>().map(|c| c.value()).unwrap_or(0) as jlong
}

/// Average<f64>.sum() — paired with `nativeGetAverageCount` because the Rust side exposes the
/// running sum/count rather than a single `mean()` method (see the `tdigest_quantiles.rs`
/// example: `avg.sum() / avg.count() as f64`). The Java `AggregateSet.getAverage()` wrapper does
/// that division for you.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_AggregateSet_nativeGetAverageSum(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jdouble {
    let set: &AggregateSet = unsafe { borrow(handle) };
    set.get::<Average<f64>>().map(|a| a.sum()).unwrap_or(0.0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_AggregateSet_nativeGetAverageCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let set: &AggregateSet = unsafe { borrow(handle) };
    set.get::<Average<f64>>().map(|a| a.count()).unwrap_or(0) as jlong
}

/// Min<f64>.value(). `Min`'s own value is `Option<f64>` (empty until the first sample), so this
/// writes "did we have a value" into the returned boolean and the value itself into `out[0]` —
/// the Java wrapper turns that into an `OptionalDouble`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_AggregateSet_nativeGetMin(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    out: jni::objects::JDoubleArray,
) -> jboolean {
    write_optional(env, handle, out, |set: &AggregateSet| {
        set.get::<Min<f64>>().and_then(|m| m.value())
    })
}

/// Max<f64>.value() — same contract as [`Java_com_accreta_AggregateSet_nativeGetMin`].
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_AggregateSet_nativeGetMax(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    out: jni::objects::JDoubleArray,
) -> jboolean {
    write_optional(env, handle, out, |set: &AggregateSet| {
        set.get::<Max<f64>>().and_then(|m| m.value())
    })
}

fn write_optional(
    env: JNIEnv,
    handle: jlong,
    out: jni::objects::JDoubleArray,
    lookup: impl FnOnce(&AggregateSet) -> Option<f64>,
) -> jboolean {
    let set: &AggregateSet = unsafe { borrow(handle) };
    match lookup(set) {
        Some(value) => {
            env.set_double_array_region(&out, 0, &[value])
                .expect("writing out-param");
            JNI_TRUE
        }
        None => JNI_FALSE,
    }
}

/// TDigest.quantile(q). Returns `f64::NAN` (TDigest's own answer for "no samples yet") if
/// `TDigest` wasn't registered on this measure.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_AggregateSet_nativeGetQuantile(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    q: jdouble,
) -> jdouble {
    let set: &AggregateSet = unsafe { borrow(handle) };
    set.get::<TDigest>()
        .map(|d| d.quantile(q))
        .unwrap_or(f64::NAN)
}
