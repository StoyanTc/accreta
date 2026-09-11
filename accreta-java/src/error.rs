//! Maps `accreta::errors::{SchemaError, IngestError}` onto thrown Java exceptions.
//!
//! JNI has no `Result` — a failed native call either returns a sentinel or throws. We throw, so
//! Java callers get a normal checked exception (`SchemaException` / `IngestException`) rather
//! than having to inspect a magic return value. Every `nativeXxx` method that can fail declares
//! `throws` on the Java side to match.

use jni::JNIEnv;

use accreta::errors::{IngestError, SchemaError};

const SCHEMA_EXCEPTION: &str = "com/accreta/SchemaException";
const INGEST_EXCEPTION: &str = "com/accreta/IngestException";

/// Throw a `SchemaException` carrying `err`'s `Display` message, then return a caller-supplied
/// sentinel (typically `0` for a `jlong`-returning native method).
///
/// Throwing from native code doesn't unwind Rust or stop execution of the current function — the
/// exception is only *pending* until control returns to the JVM. Callers must return immediately
/// after calling this (which the `sentinel` return value is for) rather than continuing to use
/// any handle involved in the failed call.
pub fn throw_schema_error<T>(env: &mut JNIEnv, err: SchemaError, sentinel: T) -> T {
    let _ = env.throw_new(SCHEMA_EXCEPTION, err.to_string());
    sentinel
}

/// Same as [`throw_schema_error`] but for `IngestError`.
pub fn throw_ingest_error<T>(env: &mut JNIEnv, err: IngestError, sentinel: T) -> T {
    let _ = env.throw_new(INGEST_EXCEPTION, err.to_string());
    sentinel
}
