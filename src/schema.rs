use std::ffi::CStr;
use std::os::raw::c_char;

use accreta::aggregate_set::{Schema, SchemaBuilder};
use accreta::aggregates::{Average, Count, Max, Min, Sum};
use accreta::measures::MeasureId;

use crate::AccretaAggregateKind;
use crate::error::{AccretaStatus, fail};
use crate::ffi_guard;
use crate::types::AccretaMeasureType;

/// Opaque handle to an in-progress [`accreta::aggregate_set::SchemaBuilder`].
///
/// Build one with [`accreta_schema_builder_new`], add every dimension and measure, then finish
/// it with [`accreta_schema_builder_build`], which consumes it. If you abandon a builder without
/// calling `build`, free it with [`accreta_schema_builder_free`] instead.
pub struct AccretaSchemaBuilder(SchemaBuilder);

/// Opaque handle to a built, immutable [`accreta::aggregate_set::Schema`].
///
/// Cheap to keep around: internally it's reference-counted, so [`accreta_engine_new`] clones it
/// rather than consuming your handle — free your `AccretaSchema*` with
/// [`accreta_schema_free`] whenever you're done with it, independently of any engines built from
/// it.
pub struct AccretaSchema(pub(crate) Schema);

/// Converts a C string into a Rust `&'static str` by leaking its backing allocation.
///
/// `accreta::aggregate_set::Schema` requires `&'static str` for dimension and measure names (see
/// `SchemaBuilder::dimension` / `SchemaBuilder::measure`) — schemas are normally built once,
/// early, and kept for a process's whole lifetime, so leaking the (small, one-time) name strings
/// here is the standard, deliberate trade-off for a stable C ABI rather than a bug.
unsafe fn leak_c_str(s: *const c_char) -> Result<&'static str, AccretaStatus> {
    if s.is_null() {
        return Err(fail(AccretaStatus::NullPointer, "name pointer is null"));
    }
    let cstr = unsafe { CStr::from_ptr(s) };
    let owned = cstr
        .to_str()
        .map_err(|_| fail(AccretaStatus::InvalidUtf8, "name is not valid UTF-8"))?
        .to_owned();
    Ok(Box::leak(owned.into_boxed_str()))
}

/// Creates a new, empty schema builder.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_schema_builder_new() -> *mut AccretaSchemaBuilder {
    ffi_guard(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(AccretaSchemaBuilder(Schema::builder())))
    })
}

/// Frees a builder that was never passed to [`accreta_schema_builder_build`].
///
/// Do not call this on a builder that `build` already consumed — `build` frees it for you.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_schema_builder_free(builder: *mut AccretaSchemaBuilder) {
    ffi_guard((), || {
        if !builder.is_null() {
            drop(unsafe { Box::from_raw(builder) });
        }
    })
}

/// Registers a dimension named `name`. Registering the same name twice is a no-op, matching
/// [`accreta::aggregate_set::SchemaBuilder::dimension`].
///
/// Returns [`AccretaStatus::Panic`] if more than 64 dimensions have already been registered (a
/// dimension mask can only represent 64 dimensions) — the builder should be discarded via
/// [`accreta_schema_builder_free`] after that.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_schema_builder_add_dimension(
    builder: *mut AccretaSchemaBuilder,
    name: *const c_char,
) -> AccretaStatus {
    ffi_guard(AccretaStatus::Panic, || {
        let Some(builder) = (unsafe { builder.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "builder pointer is null");
        };
        let name = match unsafe { leak_c_str(name) } {
            Ok(name) => name,
            Err(status) => return status,
        };
        builder.0.dimension(name);
        AccretaStatus::Ok
    })
}

fn register_measure_i64(
    builder: &mut SchemaBuilder,
    name: &'static str,
    kinds: &[AccretaAggregateKind],
) {
    let mut measure = builder.measure::<i64>(name);
    for kind in kinds {
        match kind {
            AccretaAggregateKind::Sum => {
                measure.with::<Sum<i64>>();
            }
            AccretaAggregateKind::Count => {
                measure.with_any::<Count>();
            }
            AccretaAggregateKind::Min => {
                measure.with::<Min<i64>>();
            }
            AccretaAggregateKind::Max => {
                measure.with::<Max<i64>>();
            }
            AccretaAggregateKind::Average => {
                measure.with::<Average<i64>>();
            }
        }
    }
}

fn register_measure_u64(
    builder: &mut SchemaBuilder,
    name: &'static str,
    kinds: &[AccretaAggregateKind],
) {
    let mut measure = builder.measure::<u64>(name);
    for kind in kinds {
        match kind {
            AccretaAggregateKind::Sum => {
                measure.with::<Sum<u64>>();
            }
            AccretaAggregateKind::Count => {
                measure.with_any::<Count>();
            }
            AccretaAggregateKind::Min => {
                measure.with::<Min<u64>>();
            }
            AccretaAggregateKind::Max => {
                measure.with::<Max<u64>>();
            }
            AccretaAggregateKind::Average => {
                measure.with::<Average<u64>>();
            }
        }
    }
}

fn register_measure_f64(
    builder: &mut SchemaBuilder,
    name: &'static str,
    kinds: &[AccretaAggregateKind],
) {
    let mut measure = builder.measure::<f64>(name);
    for kind in kinds {
        match kind {
            AccretaAggregateKind::Sum => {
                measure.with::<Sum<f64>>();
            }
            AccretaAggregateKind::Count => {
                measure.with_any::<Count>();
            }
            AccretaAggregateKind::Min => {
                measure.with::<Min<f64>>();
            }
            AccretaAggregateKind::Max => {
                measure.with::<Max<f64>>();
            }
            AccretaAggregateKind::Average => {
                measure.with::<Average<f64>>();
            }
        }
    }
}

/// Registers a measure named `name` with numeric type `measure_type`, and attaches every
/// aggregate kind in `kinds` (`kinds_len` entries; pass `kinds = NULL, kinds_len = 0` for a
/// measure tracked by no built-in aggregate) to it in one call.
///
/// This exists as a single call — rather than mirroring `SchemaBuilder::measure` returning a
/// separate builder handle — because `MeasureBuilder` borrows the `SchemaBuilder` it came from,
/// and that borrow can't be expressed as a second, independently-freed C handle.
///
/// Returns [`AccretaStatus::Panic`] if `name` duplicates an already-registered measure, or if the
/// same aggregate kind appears more than once in `kinds` — both are program-logic errors on the
/// caller's part, matching the panics in the underlying Rust API.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_schema_builder_add_measure(
    builder: *mut AccretaSchemaBuilder,
    name: *const c_char,
    measure_type: AccretaMeasureType,
    kinds: *const AccretaAggregateKind,
    kinds_len: usize,
) -> AccretaStatus {
    ffi_guard(AccretaStatus::Panic, || {
        let Some(builder) = (unsafe { builder.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "builder pointer is null");
        };
        let name = match unsafe { leak_c_str(name) } {
            Ok(name) => name,
            Err(status) => return status,
        };
        if kinds_len > 0 && kinds.is_null() {
            return fail(
                AccretaStatus::NullPointer,
                "kinds pointer is null but kinds_len > 0",
            );
        }
        let kinds: &[AccretaAggregateKind] = if kinds_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(kinds, kinds_len) }
        };

        match measure_type {
            AccretaMeasureType::I64 => register_measure_i64(&mut builder.0, name, kinds),
            AccretaMeasureType::U64 => register_measure_u64(&mut builder.0, name, kinds),
            AccretaMeasureType::F64 => register_measure_f64(&mut builder.0, name, kinds),
        }

        AccretaStatus::Ok
    })
}

/// Finishes building a schema, consuming and freeing `builder` either way.
///
/// On success, `*out_schema` is set to a new [`AccretaSchema`] handle the caller owns (free it
/// with [`accreta_schema_free`]). On failure, `*out_schema` is set to null and a status
/// describing why (no dimension / no measure registered) is returned; see
/// [`accreta_last_error_message`].
#[unsafe(no_mangle)]
pub extern "C" fn accreta_schema_builder_build(
    builder: *mut AccretaSchemaBuilder,
    out_schema: *mut *mut AccretaSchema,
) -> AccretaStatus {
    ffi_guard(AccretaStatus::Panic, || {
        if builder.is_null() {
            return fail(AccretaStatus::NullPointer, "builder pointer is null");
        }
        if out_schema.is_null() {
            return fail(AccretaStatus::NullPointer, "out_schema pointer is null");
        }
        let builder = unsafe { Box::from_raw(builder) };
        unsafe { *out_schema = std::ptr::null_mut() };

        match builder.0.build() {
            Ok(schema) => {
                unsafe { *out_schema = Box::into_raw(Box::new(AccretaSchema(schema))) };
                AccretaStatus::Ok
            }
            Err(err) => fail(AccretaStatus::Schema, err.to_string()),
        }
    })
}

/// Frees a schema handle. Does not affect any [`AccretaEngine`](crate::AccretaEngine) already
/// built from it — engines hold their own clone.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_schema_free(schema: *mut AccretaSchema) {
    ffi_guard((), || {
        if !schema.is_null() {
            drop(unsafe { Box::from_raw(schema) });
        }
    })
}

/// The number of dimensions registered in `schema`.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_schema_dimension_count(schema: *const AccretaSchema) -> usize {
    ffi_guard(0, || match unsafe { schema.as_ref() } {
        Some(schema) => schema.0.dimension_count(),
        None => 0,
    })
}

/// The number of measures registered in `schema`.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_schema_measure_count(schema: *const AccretaSchema) -> usize {
    ffi_guard(0, || match unsafe { schema.as_ref() } {
        Some(schema) => schema.0.measure_count(),
        None => 0,
    })
}

/// The numeric type of measure `measure_id`, written to `*out_type`.
///
/// Returns [`AccretaStatus::NotFound`] if `measure_id` isn't registered in this schema.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_schema_measure_type(
    schema: *const AccretaSchema,
    measure_id: u8,
    out_type: *mut AccretaMeasureType,
) -> AccretaStatus {
    ffi_guard(AccretaStatus::Panic, || {
        let Some(schema) = (unsafe { schema.as_ref() }) else {
            return fail(AccretaStatus::NullPointer, "schema pointer is null");
        };
        let Some(out_type) = (unsafe { out_type.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "out_type pointer is null");
        };
        match schema.0.measure(MeasureId(measure_id)) {
            Some(def) => {
                *out_type = def.data_type.into();
                AccretaStatus::Ok
            }
            None => fail(
                AccretaStatus::NotFound,
                "no measure with that id in this schema",
            ),
        }
    })
}
