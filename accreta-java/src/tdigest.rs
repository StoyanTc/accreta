//! JNI natives backing `com.accreta.TDigest` as a standalone value — separate from
//! `AggregateSet::get::<TDigest>()`, which only ever gives you a `&TDigest` borrowed from a set.
//! This exists to mirror `tdigest_quantiles.rs`'s merge-order demonstration, which builds and
//! merges free-standing digests directly via `Monoid`/`Aggregator`.

use jni::JNIEnv;
use jni::objects::JClass;
use jni::sys::{jdouble, jlong};

use accreta::aggregates::TDigest;
use accreta::{Aggregator, Monoid};

use crate::handles::{borrow, borrow_mut, drop_handle, into_handle};

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_TDigest_nativeIdentity(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    into_handle(TDigest::identity())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_TDigest_nativeUpdateInPlace(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    value: jdouble,
) {
    let digest: &mut TDigest = unsafe { borrow_mut(handle) };
    digest.update_in_place(value);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_TDigest_nativeMergeInPlace(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    other_handle: jlong,
) {
    // SAFETY: `handle` and `other_handle` are distinct live handles — `AggregateSet::merge`'s own
    // panic-on-aliasing pattern doesn't apply here since `TDigest::merge_in_place` takes `&self`
    // for `other`, not another `&mut`, so no overlap is possible even if they were the same value.
    let other: &TDigest = unsafe { borrow(other_handle) };
    let digest: &mut TDigest = unsafe { borrow_mut(handle) };
    digest.merge_in_place(other);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_TDigest_nativeQuantile(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    q: jdouble,
) -> jdouble {
    let digest: &TDigest = unsafe { borrow(handle) };
    digest.quantile(q)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_TDigest_nativeDrop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    unsafe { drop_handle::<TDigest>(handle) };
}
