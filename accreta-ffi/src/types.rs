use accreta::bucket::BucketLevel;
use accreta::measures::{MeasureType, MeasureValue};
use chrono::{DateTime, TimeZone, Utc};

use crate::error::AccretaStatus;

/// Mirrors [`accreta::measures::MeasureType`] — the numeric type of a measure's values.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccretaMeasureType {
    I64 = 0,
    U64 = 1,
    F64 = 2,
}

impl From<MeasureType> for AccretaMeasureType {
    fn from(value: MeasureType) -> Self {
        match value {
            MeasureType::I64 => AccretaMeasureType::I64,
            MeasureType::U64 => AccretaMeasureType::U64,
            MeasureType::F64 => AccretaMeasureType::F64,
        }
    }
}

/// Mirrors [`accreta::bucket::BucketLevel`] — a granularity in the rollup hierarchy.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccretaBucketLevel {
    Minute = 0,
    Hour = 1,
    Day = 2,
    Week = 3,
    Month = 4,
    Year = 5,
}

impl From<AccretaBucketLevel> for BucketLevel {
    fn from(value: AccretaBucketLevel) -> Self {
        match value {
            AccretaBucketLevel::Minute => BucketLevel::Minute,
            AccretaBucketLevel::Hour => BucketLevel::Hour,
            AccretaBucketLevel::Day => BucketLevel::Day,
            AccretaBucketLevel::Week => BucketLevel::Week,
            AccretaBucketLevel::Month => BucketLevel::Month,
            AccretaBucketLevel::Year => BucketLevel::Year,
        }
    }
}

impl From<BucketLevel> for AccretaBucketLevel {
    fn from(value: BucketLevel) -> Self {
        match value {
            BucketLevel::Minute => AccretaBucketLevel::Minute,
            BucketLevel::Hour => AccretaBucketLevel::Hour,
            BucketLevel::Day => AccretaBucketLevel::Day,
            BucketLevel::Week => AccretaBucketLevel::Week,
            BucketLevel::Month => AccretaBucketLevel::Month,
            BucketLevel::Year => AccretaBucketLevel::Year,
        }
    }
}

/// The fixed set of built-in aggregate kinds this crate exposes across the C ABI.
///
/// This intentionally does not (and cannot) grow to cover custom `Aggregator`/`Monoid`
/// implementations defined only in Rust — see the crate-level docs.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccretaAggregateKind {
    Sum = 0,
    Count = 1,
    Min = 2,
    Max = 3,
    Average = 4,
    /// Approximate quantile sketch ([`accreta::aggregates::TDigest`]).
    ///
    /// Unlike every other kind here, `TDigest` doesn't reduce to one scalar value, so it is the
    /// one kind [`crate::accreta_aggregate_set_get_value`] refuses (with
    /// [`AccretaStatus::TypeMismatch`](crate::AccretaStatus::TypeMismatch)) rather than answers.
    /// Read it with [`crate::accreta_aggregate_set_get_quantile`] instead, passing the quantile
    /// you want. It's also the only built-in aggregate that isn't generic over the measure's
    /// declared [`AccretaMeasureType`] — internally it always computes in `f64` — but that means
    /// the reverse of what you might expect: rather than working with every
    /// [`AccretaMeasureType`], it can *only* be attached to an `F64` measure via
    /// [`crate::accreta_schema_builder_add_measure`], since that call requires an exact match
    /// between the aggregate's input type and the measure's declared type, and `TDigest`'s input
    /// is fixed at `f64`. Requesting it on an `I64`/`U64` measure fails with
    /// [`AccretaStatus::TypeMismatch`](crate::AccretaStatus::TypeMismatch).
    TDigest = 5,
}

/// Untagged payload for [`AccretaMeasureValue`]. Read the field matching the struct's `tag`;
/// reading any other field is undefined behavior, same as a C union.
#[repr(C)]
#[derive(Clone, Copy)]
pub union AccretaMeasureValueData {
    pub i64: i64,
    pub u64: u64,
    pub f64: f64,
}

/// A runtime-typed numeric value: either a raw measure input, or an extracted aggregate result.
///
/// `tag` says which field of `value` is active. `Count`'s value is always `u64`; `Average`'s is
/// always `f64`; `Sum`/`Min`/`Max` match the measure's own [`AccretaMeasureType`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AccretaMeasureValue {
    pub tag: AccretaMeasureType,
    pub value: AccretaMeasureValueData,
}

impl AccretaMeasureValue {
    pub(crate) fn i64(v: i64) -> Self {
        Self {
            tag: AccretaMeasureType::I64,
            value: AccretaMeasureValueData { i64: v },
        }
    }
    pub(crate) fn u64(v: u64) -> Self {
        Self {
            tag: AccretaMeasureType::U64,
            value: AccretaMeasureValueData { u64: v },
        }
    }
    pub(crate) fn f64(v: f64) -> Self {
        Self {
            tag: AccretaMeasureType::F64,
            value: AccretaMeasureValueData { f64: v },
        }
    }
}

/// Converts a raw C measure value into accreta's [`MeasureValue`] using its own `tag` — used for
/// ingest, where accreta's `Engine::ingest` itself validates the value against the schema (see
/// `IngestError::MeasureType`), so this crate doesn't need to check it twice.
pub(crate) fn measure_value_from_raw(v: AccretaMeasureValue) -> MeasureValue {
    unsafe {
        match v.tag {
            AccretaMeasureType::I64 => MeasureValue::I64(v.value.i64),
            AccretaMeasureType::U64 => MeasureValue::U64(v.value.u64),
            AccretaMeasureType::F64 => MeasureValue::F64(v.value.f64),
        }
    }
}

/// `DateTime<Utc>` <-> milliseconds-since-Unix-epoch, the timestamp representation used
/// throughout this C ABI.
pub(crate) fn ms_to_datetime(ms: i64) -> Result<DateTime<Utc>, AccretaStatus> {
    Utc.timestamp_millis_opt(ms).single().ok_or_else(|| {
        crate::error::fail(
            AccretaStatus::InvalidArgument,
            "timestamp (ms) out of range",
        )
    })
}

pub(crate) fn datetime_to_ms(dt: DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}
