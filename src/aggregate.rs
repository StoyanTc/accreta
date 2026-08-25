use accreta::aggregate_set::AggregateSet;
use accreta::aggregates::{Average, Count, Max, Min, Sum};
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
    }
}

/// Reads the value of `kind` for `set` (whose owning measure has type `measure_type`) into
/// `*out_value`.
///
/// Returns [`AccretaStatus::TypeMismatch`] if `kind` was never attached to this measure in the
/// schema (e.g. asking for `Average` on a measure only registered with `Sum` and `Count`), or if
/// `kind` is `Min`/`Max` and no sample has been folded into this state yet.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_aggregate_set_get_value(
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

/// Frees an aggregate-set handle.
#[unsafe(no_mangle)]
pub extern "C" fn accreta_aggregate_set_free(set: *mut AccretaAggregateSet) {
    ffi_guard((), || {
        if !set.is_null() {
            drop(unsafe { Box::from_raw(set) });
        }
    })
}

/// The number of measures represented in `list` (one [`AccretaAggregateSet`] per measure, in
/// schema registration order).
#[unsafe(no_mangle)]
pub extern "C" fn accreta_aggregate_set_list_len(list: *const AccretaAggregateSetList) -> usize {
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
#[unsafe(no_mangle)]
pub extern "C" fn accreta_aggregate_set_list_get(
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
#[unsafe(no_mangle)]
pub extern "C" fn accreta_aggregate_set_list_free(list: *mut AccretaAggregateSetList) {
    ffi_guard((), || {
        if !list.is_null() {
            drop(unsafe { Box::from_raw(list) });
        }
    })
}
