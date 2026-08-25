use accreta::bucket::Bucket;
use accreta::dimensions::DimensionKey;

use crate::aggregate::AccretaAggregateSetList;
use crate::dimension_key::AccretaDimensionKey;
use crate::error::{AccretaStatus, fail};
use crate::ffi_guard;
use crate::types::{AccretaBucketLevel, datetime_to_ms};

/// Opaque handle to an owned, cloned [`accreta::bucket::Bucket`].
///
/// Obtained from [`crate::accreta_engine_bucket`] or an [`AccretaBucketCursor`]. It's a snapshot
/// — independent of the [`crate::AccretaEngine`] it came from, so later ingestion or rollups on
/// that engine won't be reflected in a bucket handle you're already holding. Free with
/// [`accreta_bucket_free`].
pub struct AccretaBucket(pub(crate) Bucket);

/// The granularity of `bucket`.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_bucket_level(bucket: *const AccretaBucket) -> AccretaBucketLevel {
    ffi_guard(AccretaBucketLevel::Minute, || {
        match unsafe { bucket.as_ref() } {
            Some(bucket) => bucket.0.level().into(),
            None => AccretaBucketLevel::Minute,
        }
    })
}

/// The inclusive start of `bucket`'s time window, in milliseconds since the Unix epoch (UTC).
#[unsafe(no_mangle)]
pub extern "C" fn accreta_bucket_start_ms(bucket: *const AccretaBucket) -> i64 {
    ffi_guard(0, || match unsafe { bucket.as_ref() } {
        Some(bucket) => datetime_to_ms(bucket.0.start()),
        None => 0,
    })
}

/// The exclusive end of `bucket`'s time window, in milliseconds since the Unix epoch (UTC).
#[unsafe(no_mangle)]
pub extern "C" fn accreta_bucket_end_ms(bucket: *const AccretaBucket) -> i64 {
    ffi_guard(0, || match unsafe { bucket.as_ref() } {
        Some(bucket) => datetime_to_ms(bucket.0.end()),
        None => 0,
    })
}

/// The number of distinct full-dimension groups in `bucket`.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_bucket_group_count(bucket: *const AccretaBucket) -> usize {
    ffi_guard(0, || match unsafe { bucket.as_ref() } {
        Some(bucket) => bucket.0.group_count(),
        None => 0,
    })
}

/// Frees a bucket handle.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_bucket_free(bucket: *mut AccretaBucket) {
    ffi_guard((), || {
        if !bucket.is_null() {
            drop(unsafe { Box::from_raw(bucket) });
        }
    })
}

/// Opaque cursor over a snapshot of a [`AccretaBucket`]'s dimension groups.
///
/// Cloned eagerly at construction time (see [`accreta_bucket_groups_cursor`]) rather than
/// borrowing the bucket, so it's safe to keep iterating even if you free the originating
/// `AccretaBucket` first.
pub struct AccretaGroupCursor {
    items: std::vec::IntoIter<(DimensionKey, Vec<accreta::aggregate_set::AggregateSet>)>,
}

/// Creates a cursor over every dimension group in `bucket`, in unspecified order.
///
/// Free it with [`accreta_group_cursor_free`] once done (whether or not you consumed every item).
#[unsafe(no_mangle)]
pub extern "C" fn accreta_bucket_groups_cursor(
    bucket: *const AccretaBucket,
) -> *mut AccretaGroupCursor {
    ffi_guard(std::ptr::null_mut(), || {
        let Some(bucket) = (unsafe { bucket.as_ref() }) else {
            return std::ptr::null_mut();
        };
        let items: Vec<_> = bucket
            .0
            .groups()
            .map(|(key, sets)| (key.clone(), sets.clone()))
            .collect();
        Box::into_raw(Box::new(AccretaGroupCursor {
            items: items.into_iter(),
        }))
    })
}

/// Advances `cursor`, writing the next group's key and aggregate-set list to `*out_key` /
/// `*out_sets` (both newly owned — free them with [`accreta_dimension_key_free`] /
/// [`accreta_aggregate_set_list_free`]).
///
/// Returns `true` and writes the pair, or returns `false` (leaving the out params untouched) once
/// the cursor is exhausted.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_group_cursor_next(
    cursor: *mut AccretaGroupCursor,
    out_key: *mut *mut AccretaDimensionKey,
    out_sets: *mut *mut AccretaAggregateSetList,
) -> bool {
    ffi_guard(false, || {
        let Some(cursor) = (unsafe { cursor.as_mut() }) else {
            fail(AccretaStatus::NullPointer, "cursor pointer is null");
            return false;
        };
        if out_key.is_null() || out_sets.is_null() {
            fail(
                AccretaStatus::NullPointer,
                "out_key/out_sets pointer is null",
            );
            return false;
        }
        match cursor.items.next() {
            Some((key, sets)) => {
                unsafe {
                    *out_key = Box::into_raw(Box::new(AccretaDimensionKey(key)));
                    *out_sets = Box::into_raw(Box::new(AccretaAggregateSetList(sets)));
                }
                true
            }
            None => false,
        }
    })
}

/// Frees a group cursor. Does not affect items already produced by [`accreta_group_cursor_next`]
/// — free those separately.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_group_cursor_free(cursor: *mut AccretaGroupCursor) {
    ffi_guard((), || {
        if !cursor.is_null() {
            drop(unsafe { Box::from_raw(cursor) });
        }
    })
}
