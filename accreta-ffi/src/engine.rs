use std::ffi::CStr;
use std::os::raw::c_char;

use accreta::dimensions::DimensionKey;
use accreta::engine::Engine;
use accreta::measures::MeasureId;
use accreta::retention::Retention;
use chrono::Duration;

use crate::aggregate::AccretaAggregateSet;
use crate::bucket::AccretaBucket;
use crate::dimension_key::{AccretaDimensionKey, mask_from_bits};
use crate::error::{AccretaStatus, fail};
use crate::ffi_guard;
use crate::schema::AccretaSchema;
use crate::types::{
    AccretaBucketLevel, AccretaMeasureValue, measure_value_from_raw, ms_to_datetime,
};

// ---------------------------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------------------------

/// Opaque handle to an owned [`accreta::retention::Retention`] policy.
pub struct AccretaRetention(pub(crate) Retention);

/// Creates a retention policy that keeps every level forever (accreta's default).
#[unsafe(no_mangle)]
pub extern "C" fn accreta_retention_new() -> *mut AccretaRetention {
    ffi_guard(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(AccretaRetention(Retention::new())))
    })
}

/// Configures `retention` to keep buckets at `level` for at most `max_age_ms` past the newest
/// bucket currently stored at that level. Mutates `retention` in place.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_retention_keep(
    retention: *mut AccretaRetention,
    level: AccretaBucketLevel,
    max_age_ms: i64,
) -> i32 {
    ffi_guard(AccretaStatus::Panic as i32, || {
        let Some(retention) = (unsafe { retention.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "retention pointer is null") as i32;
        };
        if max_age_ms < 0 {
            return fail(
                AccretaStatus::InvalidArgument,
                "max_age_ms must be non-negative",
            ) as i32;
        }
        retention.0 = retention
            .0
            .keep(level.into(), Duration::milliseconds(max_age_ms));
        AccretaStatus::Ok as i32
    })
}

/// Frees a retention handle. Safe to free immediately after passing it to
/// [`accreta_engine_new_with_retention`] — the engine copies the policy (it's a small `Copy`
/// struct internally).
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_retention_free(retention: *mut AccretaRetention) {
    ffi_guard((), || {
        if !retention.is_null() {
            drop(unsafe { Box::from_raw(retention) });
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------------------------

/// Opaque handle to an owned [`accreta::engine::Engine`].
pub struct AccretaEngine(pub(crate) Engine);

/// Creates a new engine tracking `schema`'s aggregates, with no retention limit. `schema` is
/// borrowed and cloned internally (cheap — it's reference-counted); free your `schema` handle
/// independently whenever you're done with it.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_new(schema: *const AccretaSchema) -> *mut AccretaEngine {
    ffi_guard(std::ptr::null_mut(), || {
        let Some(schema) = (unsafe { schema.as_ref() }) else {
            return std::ptr::null_mut();
        };
        Box::into_raw(Box::new(AccretaEngine(Engine::new(schema.0.clone()))))
    })
}

/// Like [`accreta_engine_new`], but discarding buckets per `retention` whenever
/// [`accreta_engine_prune`] is called.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_new_with_retention(
    schema: *const AccretaSchema,
    retention: *const AccretaRetention,
) -> *mut AccretaEngine {
    ffi_guard(std::ptr::null_mut(), || {
        let (Some(schema), Some(retention)) =
            (unsafe { schema.as_ref() }, unsafe { retention.as_ref() })
        else {
            return std::ptr::null_mut();
        };
        Box::into_raw(Box::new(AccretaEngine(Engine::with_retention(
            schema.0.clone(),
            retention.0,
        ))))
    })
}

/// Frees an engine handle. Does not affect any [`AccretaBucket`] / [`AccretaAggregateSet`]
/// snapshots you already pulled out of it — those are independently owned.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_free(engine: *mut AccretaEngine) {
    ffi_guard((), || {
        if !engine.is_null() {
            drop(unsafe { Box::from_raw(engine) });
        }
    })
}

/// Folds one raw sample into `engine`'s minute bucket for `timestamp_ms`.
///
/// `measures` (`measures_len` entries, in `MeasureId` order) and `dimensions` (`dimensions_len`
/// NUL-terminated UTF-8 strings, in `DimensionId` order) must each match the schema's counts —
/// see [`accreta_schema_measure_count`] / [`accreta_schema_dimension_count`]. Each measure
/// value's own `tag` determines how it's interpreted; if it doesn't match that measure's declared
/// type, accreta itself rejects the sample with [`AccretaStatus::Ingest`].
///
/// # Safety
///
/// `engine` must be a valid, live [`AccretaEngine`] handle. If `measures_len > 0`,
/// `measures` must point to an array of at least `measures_len` valid values. If
/// `dimensions_len > 0`, `dimensions` must point to an array of at least
/// `dimensions_len` valid C-string pointers, and each pointer must reference a
/// NUL-terminated string valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_ingest(
    engine: *mut AccretaEngine,
    timestamp_ms: i64,
    measures: *const AccretaMeasureValue,
    measures_len: usize,
    dimensions: *const *const c_char,
    dimensions_len: usize,
) -> i32 {
    ffi_guard(AccretaStatus::Panic as i32, || {
        let Some(engine) = (unsafe { engine.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "engine pointer is null") as i32;
        };
        if (measures_len > 0 && measures.is_null()) || (dimensions_len > 0 && dimensions.is_null())
        {
            return fail(
                AccretaStatus::NullPointer,
                "measures/dimensions pointer is null",
            ) as i32;
        }

        let timestamp = match ms_to_datetime(timestamp_ms) {
            Ok(dt) => dt,
            Err(status) => return status as i32,
        };

        let measures: Vec<_> = (if measures_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(measures, measures_len) }
        })
        .iter()
        .copied()
        .map(measure_value_from_raw)
        .collect();

        let dimension_ptrs: &[*const c_char] = if dimensions_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(dimensions, dimensions_len) }
        };
        let mut dimension_values = Vec::with_capacity(dimension_ptrs.len());
        for &ptr in dimension_ptrs {
            if ptr.is_null() {
                return fail(
                    AccretaStatus::NullPointer,
                    "a dimension string pointer is null",
                ) as i32;
            }
            match unsafe { CStr::from_ptr(ptr) }.to_str() {
                Ok(s) => dimension_values.push(s.to_owned()),
                Err(_) => {
                    return fail(
                        AccretaStatus::InvalidUtf8,
                        "a dimension value is not valid UTF-8",
                    ) as i32;
                }
            }
        }

        match engine.0.ingest(timestamp, measures, dimension_values) {
            Ok(()) => AccretaStatus::Ok as i32,
            Err(err) => fail(AccretaStatus::Ingest, err.to_string()) as i32,
        }
    })
}

/// Recomputes every level above `Minute` by merging bucket states upward. Safe to call
/// repeatedly.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_rollup(engine: *mut AccretaEngine) {
    ffi_guard((), || {
        if let Some(engine) = unsafe { engine.as_mut() } {
            engine.0.rollup();
        }
    })
}

/// Discards buckets older than `engine`'s configured [`AccretaRetention`] window, per level.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_prune(engine: *mut AccretaEngine) {
    ffi_guard((), || {
        if let Some(engine) = unsafe { engine.as_mut() } {
            engine.0.prune();
        }
    })
}

/// Looks up the single bucket at `level` starting exactly at `start_ms` (already truncated to
/// `level` — see `BucketLevel::truncate` in the Rust docs), writing an owned snapshot to
/// `*out_bucket`.
///
/// Returns [`AccretaStatus::NotFound`] (leaving `*out_bucket` untouched) if no such bucket exists.
///
/// # Safety
///
/// `engine` must be null or a valid live [`AccretaEngine`] handle. `out_bucket` must be
/// non-null and point to writable storage for an [`AccretaBucket`] handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_bucket(
    engine: *const AccretaEngine,
    level: AccretaBucketLevel,
    start_ms: i64,
    out_bucket: *mut *mut AccretaBucket,
) -> i32 {
    ffi_guard(AccretaStatus::Panic as i32, || {
        let Some(engine) = (unsafe { engine.as_ref() }) else {
            return fail(AccretaStatus::NullPointer, "engine pointer is null") as i32;
        };
        let Some(out_bucket) = (unsafe { out_bucket.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "out_bucket pointer is null") as i32;
        };
        let start = match ms_to_datetime(start_ms) {
            Ok(dt) => dt,
            Err(status) => return status as i32,
        };
        match engine.0.bucket(level.into(), start) {
            Some(bucket) => {
                *out_bucket = Box::into_raw(Box::new(AccretaBucket(bucket.clone())));
                AccretaStatus::Ok as i32
            }
            None => fail(AccretaStatus::NotFound, "no bucket at that level/start") as i32,
        }
    })
}

/// How many buckets are currently stored at `level`.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_bucket_count(
    engine: *const AccretaEngine,
    level: AccretaBucketLevel,
) -> usize {
    ffi_guard(0, || match unsafe { engine.as_ref() } {
        Some(engine) => engine.0.bucket_count(level.into()),
        None => 0,
    })
}

/// Opaque cursor over a snapshot of every bucket stored at one level, in chronological order.
pub struct AccretaBucketCursor {
    items: std::vec::IntoIter<accreta::bucket::Bucket>,
}

/// Creates a cursor over every bucket currently stored at `level`. Free with
/// [`accreta_bucket_cursor_free`].
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_buckets_cursor(
    engine: *const AccretaEngine,
    level: AccretaBucketLevel,
) -> *mut AccretaBucketCursor {
    ffi_guard(std::ptr::null_mut(), || {
        let Some(engine) = (unsafe { engine.as_ref() }) else {
            return std::ptr::null_mut();
        };
        let items: Vec<_> = engine.0.buckets(level.into()).cloned().collect();
        Box::into_raw(Box::new(AccretaBucketCursor {
            items: items.into_iter(),
        }))
    })
}

/// Advances `cursor`, writing the next bucket (owned) to `*out_bucket`. Returns `false` once
/// exhausted.
///
/// # Safety
///
/// `cursor` must be null or a valid live [`AccretaBucketCursor`] handle. `out_bucket` must
/// be non-null and point to writable storage for an [`AccretaBucket`] handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_bucket_cursor_next(
    cursor: *mut AccretaBucketCursor,
    out_bucket: *mut *mut AccretaBucket,
) -> bool {
    ffi_guard(false, || {
        let (Some(cursor), Some(out_bucket)) =
            (unsafe { cursor.as_mut() }, unsafe { out_bucket.as_mut() })
        else {
            fail(
                AccretaStatus::NullPointer,
                "cursor/out_bucket pointer is null",
            );
            return false;
        };
        match cursor.items.next() {
            Some(bucket) => {
                *out_bucket = Box::into_raw(Box::new(AccretaBucket(bucket)));
                true
            }
            None => false,
        }
    })
}

/// Frees a bucket cursor.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_bucket_cursor_free(cursor: *mut AccretaBucketCursor) {
    ffi_guard((), || {
        if !cursor.is_null() {
            drop(unsafe { Box::from_raw(cursor) });
        }
    })
}

/// Merges every bucket at `level` overlapping `[range_start_ms, range_end_ms)` into one total
/// [`AccretaAggregateSet`] for `measure_id`, across every dimension group — i.e. the ungrouped
/// total. Writes the owned result to `*out_set`.
///
/// # Safety
///
/// `engine` must be null or a valid live [`AccretaEngine`] handle. `out_set` must be non-null
/// and point to writable storage for an [`AccretaAggregateSet`] handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_query_range(
    engine: *const AccretaEngine,
    level: AccretaBucketLevel,
    range_start_ms: i64,
    range_end_ms: i64,
    measure_id: u8,
    out_set: *mut *mut AccretaAggregateSet,
) -> i32 {
    ffi_guard(AccretaStatus::Panic as i32, || {
        let Some(engine) = (unsafe { engine.as_ref() }) else {
            return fail(AccretaStatus::NullPointer, "engine pointer is null") as i32;
        };
        let Some(out_set) = (unsafe { out_set.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "out_set pointer is null") as i32;
        };
        let (start, end) = match (ms_to_datetime(range_start_ms), ms_to_datetime(range_end_ms)) {
            (Ok(s), Ok(e)) => (s, e),
            (Err(status), _) | (_, Err(status)) => return status as i32,
        };
        match engine
            .0
            .query_range(level.into(), start, end, MeasureId(measure_id))
        {
            Ok(set) => {
                *out_set = Box::into_raw(Box::new(AccretaAggregateSet(set)));
                AccretaStatus::Ok as i32
            }
            Err(err) => fail(AccretaStatus::NotFound, err.to_string()) as i32,
        }
    })
}

/// Opaque cursor over the result of [`accreta_engine_query_range_grouped`]: one
/// `(dimension key, aggregate set)` pair per distinct value of the projected `group_by` mask.
pub struct AccretaGroupedQueryCursor {
    items: std::vec::IntoIter<(DimensionKey, accreta::aggregate_set::AggregateSet)>,
}

/// Like [`accreta_engine_query_range`], but grouped by the dimensions selected by `group_by_bits`
/// (bit `i` selects `DimensionId(i)`; `0` groups by nothing, producing one overall total). Writes
/// a cursor over the grouped results to `*out_cursor`.
///
/// # Safety
///
/// `engine` must be null or a valid live [`AccretaEngine`] handle. `out_cursor` must be non-null
/// and point to writable storage for an [`AccretaGroupedQueryCursor`] handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_engine_query_range_grouped(
    engine: *const AccretaEngine,
    level: AccretaBucketLevel,
    range_start_ms: i64,
    range_end_ms: i64,
    measure_id: u8,
    group_by_bits: u64,
    out_cursor: *mut *mut AccretaGroupedQueryCursor,
) -> i32 {
    ffi_guard(AccretaStatus::Panic as i32, || {
        let Some(engine) = (unsafe { engine.as_ref() }) else {
            return fail(AccretaStatus::NullPointer, "engine pointer is null") as i32;
        };
        let Some(out_cursor) = (unsafe { out_cursor.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "out_cursor pointer is null") as i32;
        };
        let (start, end) = match (ms_to_datetime(range_start_ms), ms_to_datetime(range_end_ms)) {
            (Ok(s), Ok(e)) => (s, e),
            (Err(status), _) | (_, Err(status)) => return status as i32,
        };
        match engine.0.query_range_grouped(
            level.into(),
            start,
            end,
            MeasureId(measure_id),
            mask_from_bits(group_by_bits),
        ) {
            Ok(result) => {
                let items: Vec<_> = result.into_iter().collect();
                *out_cursor = Box::into_raw(Box::new(AccretaGroupedQueryCursor {
                    items: items.into_iter(),
                }));
                AccretaStatus::Ok as i32
            }
            Err(err) => fail(AccretaStatus::NotFound, err.to_string()) as i32,
        }
    })
}

/// Advances `cursor`, writing the next `(key, set)` pair (both owned) to `*out_key` / `*out_set`.
/// Returns `false` once exhausted.
///
/// # Safety
///
/// `cursor` must be null or a valid live [`AccretaGroupedQueryCursor`] handle. When non-null,
/// `out_key` and `out_set` must point to writable storage for the returned handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_grouped_query_cursor_next(
    cursor: *mut AccretaGroupedQueryCursor,
    out_key: *mut *mut AccretaDimensionKey,
    out_set: *mut *mut AccretaAggregateSet,
) -> bool {
    ffi_guard(false, || {
        let Some(cursor) = (unsafe { cursor.as_mut() }) else {
            fail(AccretaStatus::NullPointer, "cursor pointer is null");
            return false;
        };
        if out_key.is_null() || out_set.is_null() {
            fail(
                AccretaStatus::NullPointer,
                "out_key/out_set pointer is null",
            );
            return false;
        }
        match cursor.items.next() {
            Some((key, set)) => {
                unsafe {
                    *out_key = Box::into_raw(Box::new(AccretaDimensionKey(key)));
                    *out_set = Box::into_raw(Box::new(AccretaAggregateSet(set)));
                }
                true
            }
            None => false,
        }
    })
}

/// Frees a grouped-query cursor.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_grouped_query_cursor_free(cursor: *mut AccretaGroupedQueryCursor) {
    ffi_guard((), || {
        if !cursor.is_null() {
            drop(unsafe { Box::from_raw(cursor) });
        }
    })
}
