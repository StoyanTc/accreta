use accreta::aggregate_set::AggregateSet;
use accreta::aggregates::{Average, Count, Max, Min, Sum, TDigest};
use accreta::measures::MeasureType;

use crate::error::{AccretaStatus, fail};
use crate::ffi_guard;
use crate::types::AccretaMeasureValue;
use crate::{AccretaAggregateKind, AccretaMeasureType};

/// Opaque handle to an owned [`accreta::aggregate_set::AggregateSet`] — the states of every
/// aggregate attached to one measure, for one dimension group.
///
/// Obtained from [`crate::accreta_engine_query_range`], a grouped-query cursor, or a bucket-group
/// cursor. Always owned by the caller; free it with [`accreta_aggregate_set_free`].
pub struct AccretaAggregateSet(pub(crate) AggregateSet);

/// Opaque handle to an owned list of [`AccretaAggregateSet`], one per measure in registration
/// order — what a single dimension group in a [`crate::AccretaBucket`] holds.
pub struct AccretaAggregateSetList(pub(crate) Vec<AggregateSet>);

/// Extracts the value of `kind` from `set`, given the measure's declared `measure_type` (get it
/// from [`crate::accreta_schema_measure_type`]).
///
/// # Assumption on the built-in aggregates' shape
///
/// This function assumes, following the pattern shown by `accreta`'s own tests
/// (`set.get::<Sum<f64>>().unwrap().value()`): `Sum<T>`/`Min<T>`/`Max<T>` are generic over the
/// measure's numeric type `T` and expose `.value() -> T` (for `Min`/`Max`, `Option<T>`, `None` at
/// the identity / "no samples seen" state — there is no other principled identity element for a
/// min/max monoid); `Count` is non-generic with `.value() -> u64`; `Average` is non-generic over
/// its *aggregate* type but generic over the measure's `T` for its `Aggregator` impl, with
/// `.value() -> f64` regardless of `T`. **Verify this against your actual `aggregates.rs`** —
/// this is the one place in this crate that had to be written without seeing that file, and it's
/// deliberately the only function that would need to change if a signature differs.
///
/// `TDigest` is deliberately excluded from this function's `(kind, measure_type)` match — it
/// doesn't reduce to a single scalar `AccretaMeasureValue`, so [`accreta_aggregate_set_get_value`]
/// rejects it before ever calling in here (see that function's early check). The `K::TDigest`
/// arms below exist only so the match stays exhaustive; they are unreachable via the public FFI
/// entry point.
fn extract_value(
    set: &AggregateSet,
    kind: AccretaAggregateKind,
    measure_type: MeasureType,
) -> Option<AccretaMeasureValue> {
    use AccretaAggregateKind as K;
    use MeasureType as M;
    match (kind, measure_type) {
        (K::Sum, M::I64) => set
            .get::<Sum<i64>>()
            .map(|s| AccretaMeasureValue::i64(s.value())),
        (K::Sum, M::U64) => set
            .get::<Sum<u64>>()
            .map(|s| AccretaMeasureValue::u64(s.value())),
        (K::Sum, M::F64) => set
            .get::<Sum<f64>>()
            .map(|s| AccretaMeasureValue::f64(s.value())),

        (K::Count, M::I64 | M::U64 | M::F64) => set
            .get::<Count>()
            .map(|s| AccretaMeasureValue::u64(s.value())),

        (K::Min, M::I64) => set
            .get::<Min<i64>>()
            .and_then(|s| s.value())
            .map(AccretaMeasureValue::i64),
        (K::Min, M::U64) => set
            .get::<Min<u64>>()
            .and_then(|s| s.value())
            .map(AccretaMeasureValue::u64),
        (K::Min, M::F64) => set
            .get::<Min<f64>>()
            .and_then(|s| s.value())
            .map(AccretaMeasureValue::f64),

        (K::Max, M::I64) => set
            .get::<Max<i64>>()
            .and_then(|s| s.value())
            .map(AccretaMeasureValue::i64),
        (K::Max, M::U64) => set
            .get::<Max<u64>>()
            .and_then(|s| s.value())
            .map(AccretaMeasureValue::u64),
        (K::Max, M::F64) => set
            .get::<Max<f64>>()
            .and_then(|s| s.value())
            .map(AccretaMeasureValue::f64),

        (K::Average, M::I64) => set
            .get::<Average<i64>>()
            .map(|s| AccretaMeasureValue::f64((s.sum() / s.count() as i64) as f64)),
        (K::Average, M::U64) => set
            .get::<Average<u64>>()
            .map(|s| AccretaMeasureValue::f64((s.sum() / s.count()) as f64)),
        (K::Average, M::F64) => set
            .get::<Average<f64>>()
            .map(|s| AccretaMeasureValue::f64(s.sum() / s.count() as f64)),

        (K::TDigest, M::I64 | M::U64 | M::F64) => None,
    }
}

/// Reads the value of `kind` for `set` (whose owning measure has type `measure_type`) into
/// `*out_value`.
///
/// Returns [`AccretaStatus::TypeMismatch`] if `kind` was never attached to this measure in the
/// schema (e.g. asking for `Average` on a measure only registered with `Sum` and `Count`), or if
/// `kind` is `Min`/`Max` and no sample has been folded into this state yet.
///
/// `kind = AccretaAggregateKind::TDigest` always fails with
/// [`AccretaStatus::TypeMismatch`] here — a `TDigest` doesn't reduce to one scalar value. Read it
/// with [`accreta_aggregate_set_get_quantile`] instead.
///
/// # Safety
///
/// `set` must be null or a valid live [`AccretaAggregateSet`] handle. `out_value` must be
/// non-null and point to writable storage for an [`AccretaMeasureValue`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_aggregate_set_get_value(
    set: *const AccretaAggregateSet,
    kind: AccretaAggregateKind,
    measure_type: AccretaMeasureType,
    out_value: *mut AccretaMeasureValue,
) -> AccretaStatus {
    ffi_guard(AccretaStatus::Panic, || {
        let Some(set) = (unsafe { set.as_ref() }) else {
            return fail(AccretaStatus::NullPointer, "set pointer is null");
        };
        let Some(out_value) = (unsafe { out_value.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "out_value pointer is null");
        };
        if matches!(kind, AccretaAggregateKind::TDigest) {
            return fail(
                AccretaStatus::TypeMismatch,
                "TDigest has no single scalar value; use accreta_aggregate_set_get_quantile",
            );
        }
        let measure_type = match measure_type {
            AccretaMeasureType::I64 => MeasureType::I64,
            AccretaMeasureType::U64 => MeasureType::U64,
            AccretaMeasureType::F64 => MeasureType::F64,
        };
        match extract_value(&set.0, kind, measure_type) {
            Some(value) => {
                *out_value = value;
                AccretaStatus::Ok
            }
            None => fail(
                AccretaStatus::TypeMismatch,
                "aggregate kind was not registered for this measure, or holds no value yet",
            ),
        }
    })
}

/// Reads the estimated value at quantile `quantile` (clamped to `0.0..=1.0`) from the `TDigest`
/// attached to `set`, writing it to `*out_value`.
///
/// Unlike [`accreta_aggregate_set_get_value`], this takes no `measure_type` parameter: `TDigest`
/// is not generic over the measure's declared type — it's always a plain `f64` sketch — so there
/// is nothing to dispatch on. In practice the owning measure will always be `F64` anyway, since
/// [`crate::accreta_schema_builder_add_measure`] refuses to attach `TDigest` to an `I64`/`U64`
/// measure in the first place (see [`AccretaAggregateKind::TDigest`]).
///
/// Returns [`AccretaStatus::TypeMismatch`] if `TDigest` was never attached to this measure in the
/// schema. If the measure has a `TDigest` but no sample has been folded into it yet, `*out_value`
/// is set to `f64::NAN` and the status is still [`AccretaStatus::Ok`] — mirroring
/// [`accreta::aggregates::TDigest::quantile`]'s own "no samples yet" behavior, since that's a
/// valid (if unhelpful) reading rather than an error.
///
/// # Safety
///
/// `set` must be null or a valid live [`AccretaAggregateSet`] handle. `out_value` must be
/// non-null and point to writable `f64` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_aggregate_set_get_quantile(
    set: *const AccretaAggregateSet,
    quantile: f64,
    out_value: *mut f64,
) -> AccretaStatus {
    ffi_guard(AccretaStatus::Panic, || {
        let Some(set) = (unsafe { set.as_ref() }) else {
            return fail(AccretaStatus::NullPointer, "set pointer is null");
        };
        let Some(out_value) = (unsafe { out_value.as_mut() }) else {
            return fail(AccretaStatus::NullPointer, "out_value pointer is null");
        };
        match set.0.get::<TDigest>() {
            Some(digest) => {
                *out_value = digest.quantile(quantile);
                AccretaStatus::Ok
            }
            None => fail(
                AccretaStatus::TypeMismatch,
                "TDigest was not registered for this measure",
            ),
        }
    })
}

/// Frees an aggregate-set handle.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_aggregate_set_free(set: *mut AccretaAggregateSet) {
    ffi_guard((), || {
        if !set.is_null() {
            drop(unsafe { Box::from_raw(set) });
        }
    })
}

/// The number of measures represented in `list` (one [`AccretaAggregateSet`] per measure, in
/// schema registration order).
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_aggregate_set_list_len(
    list: *const AccretaAggregateSetList,
) -> usize {
    ffi_guard(0, || match unsafe { list.as_ref() } {
        Some(list) => list.0.len(),
        None => 0,
    })
}

/// Returns the aggregate set for measure `measure_index` (its position in schema registration
/// order, i.e. `MeasureId(measure_index).index()`) as a new owned handle — free it with
/// [`accreta_aggregate_set_free`], independently of `list`.
///
/// Returns null if `measure_index` is out of range.
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_aggregate_set_list_get(
    list: *const AccretaAggregateSetList,
    measure_index: usize,
) -> *mut AccretaAggregateSet {
    ffi_guard(std::ptr::null_mut(), || {
        let Some(list) = (unsafe { list.as_ref() }) else {
            return std::ptr::null_mut();
        };
        match list.0.get(measure_index) {
            Some(set) => Box::into_raw(Box::new(AccretaAggregateSet(set.clone()))),
            None => std::ptr::null_mut(),
        }
    })
}

/// Frees an aggregate-set list handle. Does not affect handles already obtained from
/// [`accreta_aggregate_set_list_get`] — those are independently owned and must be freed
/// separately via [`accreta_aggregate_set_free`].
///
/// # Safety
///
/// This function is an FFI boundary. The caller must satisfy the pointer validity
/// requirements implied by its arguments: every non-null input pointer must point to
/// a live value of the expected type, and every output pointer must be valid and writable
/// for the value written by this function. Handles passed to `*_free` must have been
/// returned by the corresponding constructor and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accreta_aggregate_set_list_free(list: *mut AccretaAggregateSetList) {
    ffi_guard((), || {
        if !list.is_null() {
            drop(unsafe { Box::from_raw(list) });
        }
    })
}
