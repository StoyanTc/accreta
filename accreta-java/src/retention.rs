//! JNI natives backing `com.accreta.Retention`.
//!
//! `Retention` is a small `Copy` value (`Engine::retention()` returns it by value, and
//! `Engine::set_retention` takes it by value) — so `nativeKeep` replaces the boxed value in
//! place via a copy-then-overwrite rather than needing ownership transfer semantics like
//! `SchemaBuilder`/`MeasureBuilder` do.

use chrono::Duration;
use jni::JNIEnv;
use jni::objects::JClass;
use jni::sys::{jint, jlong};

use accreta::bucket::BucketLevel;
use accreta::retention::Retention;

use crate::engine::bucket_level_from_ordinal;
use crate::handles::{borrow_mut, drop_handle, into_handle};

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Retention_nativeNew(_env: JNIEnv, _class: JClass) -> jlong {
    into_handle(Retention::new())
}

/// Configures how long buckets at `level` are kept, in seconds — mirrors
/// `Retention::keep(level, max_age)`. Assumes `keep` is `fn(self, BucketLevel, Duration) -> Self`
/// (a `Copy`-friendly builder method); flagged in the README as unconfirmed against your actual
/// signature.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Retention_nativeKeep(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    level_ordinal: jint,
    max_age_seconds: jlong,
) {
    let level: BucketLevel = bucket_level_from_ordinal(level_ordinal);
    let max_age = Duration::seconds(max_age_seconds);
    let slot: &mut Retention = unsafe { borrow_mut(handle) };
    *slot = slot.keep(level, max_age);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Retention_nativeDrop(_env: JNIEnv, _class: JClass, handle: jlong) {
    unsafe { drop_handle::<Retention>(handle) };
}
