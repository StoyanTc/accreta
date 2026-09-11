//! JNI natives backing `com.accreta.SchemaBuilder` and `com.accreta.Schema`.
//!
//! Kept deliberately parallel to the Rust builder: `SchemaBuilder::dimension` /
//! `SchemaBuilder::measure` / `MeasureBuilder::with*` / `MeasureBuilder::done` /
//! `SchemaBuilder::build` each get one native method, so the Java call chain reads the same as
//! the Rust one does.
//!
//! f64 is the only measure numeric type wired up so far (it's what every one of your shared
//! examples uses, and it's the only type TDigest can attach to). i64/u64 support is a follow-up:
//! same pattern as here, mirroring the `register_measure_f64/i64/u64` split you already have in
//! accreta-ffi's `schema.rs`.

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::jlong;

use accreta::aggregate_set::{MeasureBuilder, Schema, SchemaBuilder};
use accreta::aggregates::{Average, Count, Max, Min, Sum, TDigest};

use crate::error::throw_schema_error;
use crate::handles::{borrow_mut, drop_handle, into_handle, take_handle};

// ---------------------------------------------------------------------------------------------
// SchemaBuilder
// ---------------------------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_SchemaBuilder_nativeNew(_env: JNIEnv, _class: JClass) -> jlong {
    into_handle(SchemaBuilder::default())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_SchemaBuilder_nativeDimension(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    name: JString,
) {
    // `SchemaBuilder::dimension` takes `&'static str`. Java strings are transient, so we leak
    // the name the same way accreta-node does for dimension/measure names — schemas are built
    // once at startup, not per-request, so this is a fixed, bounded cost.
    let name: String = env
        .get_string(&name)
        .expect("dimension name is not a valid UTF-8 Java string")
        .into();
    let name: &'static str = Box::leak(name.into_boxed_str());

    let builder: &mut SchemaBuilder = unsafe { borrow_mut(handle) };
    builder.dimension(name);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_SchemaBuilder_nativeMeasureF64(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    name: JString,
) -> jlong {
    let name: String = env
        .get_string(&name)
        .expect("measure name is not a valid UTF-8 Java string")
        .into();
    let name: &'static str = Box::leak(name.into_boxed_str());

    let builder: &mut SchemaBuilder = unsafe { borrow_mut(handle) };
    let measure_builder: MeasureBuilder<'_, f64> = builder.measure::<f64>(name);

    // SAFETY: erasing the borrow to 'static so it can cross the FFI boundary as its own handle.
    // The borrowed `SchemaBuilder` (`handle`) must not be touched again by *any* other native
    // call until this measure-builder handle is consumed via `nativeWith*`/`nativeDone` — the
    // Java `MeasureBuilder` wrapper enforces this by holding a reference to its parent
    // `SchemaBuilder` and only returning it again from `done()`.
    let measure_builder: MeasureBuilder<'static, f64> = unsafe { std::mem::transmute(measure_builder) };
    into_handle(measure_builder)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_SchemaBuilder_nativeBuild(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let builder: SchemaBuilder = unsafe { take_handle(handle) };
    match builder.build() {
        Ok(schema) => into_handle(schema),
        Err(err) => throw_schema_error(&mut env, err, 0),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_SchemaBuilder_nativeDrop(_env: JNIEnv, _class: JClass, handle: jlong) {
    unsafe { drop_handle::<SchemaBuilder>(handle) };
}

// ---------------------------------------------------------------------------------------------
// MeasureBuilder<'static, f64>
// ---------------------------------------------------------------------------------------------

macro_rules! with_aggregate {
    ($fn_name:ident, $aggregate:ty, $method:ident) => {
        #[unsafe(no_mangle)]
        pub extern "system" fn $fn_name(_env: JNIEnv, _class: JClass, handle: jlong) {
            let builder: &mut MeasureBuilder<'static, f64> = unsafe { borrow_mut(handle) };
            builder.$method::<$aggregate>();
        }
    };
}

with_aggregate!(Java_com_accreta_MeasureBuilder_nativeWithSum, Sum<f64>, with);
with_aggregate!(Java_com_accreta_MeasureBuilder_nativeWithAverage, Average<f64>, with);
with_aggregate!(Java_com_accreta_MeasureBuilder_nativeWithMin, Min<f64>, with);
with_aggregate!(Java_com_accreta_MeasureBuilder_nativeWithMax, Max<f64>, with);
with_aggregate!(Java_com_accreta_MeasureBuilder_nativeWithTDigest, TDigest, with);
// Count ignores the value entirely, so it goes through `with_any` rather than `with`.
with_aggregate!(Java_com_accreta_MeasureBuilder_nativeWithCount, Count, with_any);

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_MeasureBuilder_nativeDone(_env: JNIEnv, _class: JClass, handle: jlong) {
    // `done(self) -> &'a mut SchemaBuilder` — we don't need the returned reference (the Java
    // `MeasureBuilder` already holds its own reference back to the owning `SchemaBuilder`
    // object), we just need this call to consume the box and end the erased borrow.
    let builder: MeasureBuilder<'static, f64> = unsafe { take_handle(handle) };
    let _ = builder.done();
}

// ---------------------------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Schema_nativeDimensionCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let schema: &Schema = unsafe { crate::handles::borrow(handle) };
    schema.dimension_count() as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Schema_nativeMeasureCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let schema: &Schema = unsafe { crate::handles::borrow(handle) };
    schema.measure_count() as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_accreta_Schema_nativeDrop(_env: JNIEnv, _class: JClass, handle: jlong) {
    unsafe { drop_handle::<Schema>(handle) };
}
