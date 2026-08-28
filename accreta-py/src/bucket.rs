//! `BucketLevel` wrapper.
//!
//! We don't expose `accreta::bucket::Bucket` itself as a `pyclass` — nothing in the query API
//! needs to hand a raw `Bucket` back to Python (`Engine.query_range`/`query_range_grouped`
//! already do the merging and hand back an `AggregateSet`/dict of them). If you need
//! bucket-level introspection (`Engine.bucket_count`, iterating raw buckets) later, add a
//! `PyBucket` then — no need to carry the extra surface area until something needs it.

use accreta::bucket::BucketLevel;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

#[pyclass(frozen, name = "BucketLevel", eq, from_py_object)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PyBucketLevel {
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl From<PyBucketLevel> for BucketLevel {
    fn from(level: PyBucketLevel) -> Self {
        match level {
            PyBucketLevel::Minute => BucketLevel::Minute,
            PyBucketLevel::Hour => BucketLevel::Hour,
            PyBucketLevel::Day => BucketLevel::Day,
            PyBucketLevel::Week => BucketLevel::Week,
            PyBucketLevel::Month => BucketLevel::Month,
            PyBucketLevel::Year => BucketLevel::Year,
        }
    }
}

impl From<BucketLevel> for PyBucketLevel {
    fn from(level: BucketLevel) -> Self {
        match level {
            BucketLevel::Minute => PyBucketLevel::Minute,
            BucketLevel::Hour => PyBucketLevel::Hour,
            BucketLevel::Day => PyBucketLevel::Day,
            BucketLevel::Week => PyBucketLevel::Week,
            BucketLevel::Month => PyBucketLevel::Month,
            BucketLevel::Year => PyBucketLevel::Year,
        }
    }
}

/// Parse a level given either a `BucketLevel` or a plain string ("minute", "hour", ...), so
/// Python callers who'd rather not import the enum can pass a string.
pub fn parse_level(value: &Bound<'_, PyAny>) -> PyResult<BucketLevel> {
    if let Ok(level) = value.extract::<PyBucketLevel>() {
        return Ok(level.into());
    }

    if let Ok(name) = value.extract::<String>() {
        return match name.to_ascii_lowercase().as_str() {
            "minute" => Ok(BucketLevel::Minute),
            "hour" => Ok(BucketLevel::Hour),
            "day" => Ok(BucketLevel::Day),
            "week" => Ok(BucketLevel::Week),
            "month" => Ok(BucketLevel::Month),
            "year" => Ok(BucketLevel::Year),
            other => Err(PyValueError::new_err(format!(
                "unknown bucket level '{other}' (expected one of: minute, hour, day, week, month, year)"
            ))),
        };
    }

    Err(PyValueError::new_err(
        "expected a BucketLevel or one of the strings: minute, hour, day, week, month, year",
    ))
}
