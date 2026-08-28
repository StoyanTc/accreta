use std::fmt;

use crate::erased::AggregateFactory;

/// Numeric types supported by measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeasureType {
    /// Signed 64-bit integer.
    I64,
    /// Unsigned 64-bit integer.
    U64,
    /// 64-bit floating point.
    F64,
}

impl fmt::Display for MeasureType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I64 => write!(f, "i64"),
            Self::U64 => write!(f, "u64"),
            Self::F64 => write!(f, "f64"),
        }
    }
}

/// Numeric types that can be used as measure values.
///
/// This is deliberately a small crate-defined trait rather than a broad
/// numeric abstraction. It describes exactly the types supported by the
/// measure system.
pub trait MeasureNumber: Copy + Default + std::fmt::Debug + Send + Sync + 'static {
    /// The [`MeasureType`] tag corresponding to this Rust type.
    const TYPE: MeasureType;

    /// Extracts a value of this type from `value`, or returns `None` if
    /// `value` holds a different [`MeasureType`].
    fn from_value(value: MeasureValue) -> Option<Self>;
}

impl MeasureNumber for i64 {
    const TYPE: MeasureType = MeasureType::I64;

    fn from_value(value: MeasureValue) -> Option<Self> {
        match value {
            MeasureValue::I64(value) => Some(value),
            _ => None,
        }
    }
}

impl MeasureNumber for u64 {
    const TYPE: MeasureType = MeasureType::U64;

    fn from_value(value: MeasureValue) -> Option<Self> {
        match value {
            MeasureValue::U64(value) => Some(value),
            _ => None,
        }
    }
}

impl MeasureNumber for f64 {
    const TYPE: MeasureType = MeasureType::F64;

    fn from_value(value: MeasureValue) -> Option<Self> {
        match value {
            MeasureValue::F64(value) => Some(value),
            _ => None,
        }
    }
}

/// Extracts an aggregator's `Input` value from a [`MeasureValue`].
///
/// Implemented for the concrete numeric types an aggregator can read
/// directly, and for `()` for aggregators that ignore the sampled value.
pub trait FromValue: Sized {
    /// Attempts to convert `value` into `Self`, returning `None` if the
    /// underlying [`MeasureType`] doesn't match.
    fn from_value(value: MeasureValue) -> Option<Self>;
}

impl FromValue for f64 {
    fn from_value(value: MeasureValue) -> Option<f64> {
        match value {
            MeasureValue::F64(v) => Some(v),
            _ => None,
        }
    }
}

impl FromValue for i64 {
    fn from_value(value: MeasureValue) -> Option<i64> {
        match value {
            MeasureValue::I64(v) => Some(v),
            _ => None,
        }
    }
}

impl FromValue for u64 {
    fn from_value(value: MeasureValue) -> Option<u64> {
        match value {
            MeasureValue::U64(v) => Some(v),
            _ => None,
        }
    }
}

/// `()` is the `Input` type for aggregators that don't care about the sampled
/// value at all (e.g. `Count`) — it accepts any `MeasureValue` unconditionally.
impl FromValue for () {
    fn from_value(_value: MeasureValue) -> Option<()> {
        Some(())
    }
}

/// Identifies a measure in the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeasureId(pub u8);

impl MeasureId {
    /// Returns this measure's position, for indexing into
    /// [`MeasureValues`]-backed storage.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Defines a named measure in the schema.
#[derive(Debug, Clone)]
pub struct MeasureDefinition {
    /// The measure's identifier within the schema.
    pub id: MeasureId,
    /// The measure's display name.
    pub name: &'static str,
    /// The numeric type of values sampled for this measure.
    pub data_type: MeasureType,
    /// The aggregators configured to run over this measure.
    pub factories: Vec<AggregateFactory>,
}

/// Runtime numeric value of a measure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MeasureValue {
    /// A signed 64-bit integer value.
    I64(i64),
    /// An unsigned 64-bit integer value.
    U64(u64),
    /// A 64-bit floating point value.
    F64(f64),
}

impl MeasureValue {
    /// Returns the [`MeasureType`] of the value held by this variant.
    pub fn data_type(self) -> MeasureType {
        match self {
            Self::I64(_) => MeasureType::I64,
            Self::U64(_) => MeasureType::U64,
            Self::F64(_) => MeasureType::F64,
        }
    }
}

/// Measure values belonging to a sample.
///
/// The position corresponds to [`MeasureId`].
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureValues {
    values: Vec<MeasureValue>,
}

impl MeasureValues {
    /// Wraps a vector of per-measure values, indexed by `MeasureId`.
    pub fn new(values: Vec<MeasureValue>) -> Self {
        Self { values }
    }

    /// Returns the value for `measure`, or `None` if this sample has no
    /// value at that position (e.g. the vector is shorter than expected).
    pub fn get(&self, measure: MeasureId) -> Option<MeasureValue> {
        self.values.get(measure.index()).copied()
    }

    /// Returns the underlying per-measure values, indexed by `MeasureId`.
    pub fn values(&self) -> &[MeasureValue] {
        &self.values
    }
}

impl From<f64> for MeasureValue {
    fn from(value: f64) -> Self {
        MeasureValue::F64(value)
    }
}

impl From<i64> for MeasureValue {
    fn from(value: i64) -> Self {
        MeasureValue::I64(value)
    }
}

impl From<u64> for MeasureValue {
    fn from(value: u64) -> Self {
        MeasureValue::U64(value)
    }
}

impl MeasureValues {
    /// Builds `MeasureValues` from a fixed-size array of any type
    /// convertible into [`MeasureValue`], in array order.
    pub fn from_array<T, const N: usize>(values: [T; N]) -> Self
    where
        T: Into<MeasureValue>,
    {
        Self {
            values: values.into_iter().map(Into::into).collect(),
        }
    }
}
