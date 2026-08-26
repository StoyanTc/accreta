use accreta::dimensions::{DimensionId, DimensionKey, DimensionMask};

use crate::error::{AccretaStatus, fail};
use crate::ffi_guard;

/// Opaque handle to an owned [`accreta::dimensions::DimensionKey`] — a concrete combination of
/// dimension values, as returned by bucket-group and grouped-query iteration.
pub struct AccretaDimensionKey(pub(crate) DimensionKey);

/// Builds a [`DimensionMask`] from a raw `u64` bitset (bit `i` selects `DimensionId(i)`),
/// entirely through `DimensionMask`'s public API — this crate never constructs one from private
/// fields.
pub(crate) fn mask_from_bits(bits: u64) -> DimensionMask {
    (0..64u8).fold(DimensionMask::new(), |mask, i| {
        if bits & (1u64 << i) != 0 {
            mask.with(DimensionId(i))
        } else {
            mask
        }
    })
}

/// The number of values in `key`.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_dimension_key_len(key: *const AccretaDimensionKey) -> usize {
    ffi_guard(0, || match unsafe { key.as_ref() } {
        Some(key) => key.0.values().len(),
        None => 0,
    })
}

/// Reads the value at `index` into `*out_value`.
///
/// Returns [`AccretaStatus::NotFound`] if `index` is out of range.
///
/// # Safety
///
/// `key` must be null or a valid live [`AccretaDimensionKey`] handle. `out_value` must be
/// non-null and point to writable `u32` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_dimension_key_get(
    key: *const AccretaDimensionKey,
    index: usize,
    out_value: *mut u32,
) -> AccretaStatus {
    ffi_guard(AccretaStatus::Panic, || {
        let Some(key) = (unsafe { key.as_ref() }) else {
            return fail(AccretaStatus::NullPointer, "key pointer is null");
        };
        let Some(out_value) = (unsafe { out_value.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "out_value pointer is null");
        };
        match key.0.values().get(index) {
            Some(value) => {
                *out_value = *value;
                AccretaStatus::Ok
            }
            None => fail(AccretaStatus::NotFound, "index out of range"),
        }
    })
}

/// Frees a dimension key handle returned by a group or grouped-query cursor.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_dimension_key_free(key: *mut AccretaDimensionKey) {
    ffi_guard((), || {
        if !key.is_null() {
            drop(unsafe { Box::from_raw(key) });
        }
    })
}
