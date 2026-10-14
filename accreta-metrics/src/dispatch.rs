//! Bridges the HTTP layer's runtime (string-typed) schema/query shapes onto `accreta`'s
//! compile-time-generic API.
//!
//! Two things about `accreta`'s public API drive everything in this file (as of `accreta` 0.4.1):
//!
//! 1. `TDigest::Aggregator::Input = f64` unconditionally, so `MeasureBuilder<T>::with::<TDigest>()`
//!    only compiles for `T = f64`. There is no generic path that also covers `TDigest`, which is
//!    why the three value types below get separate, explicit match arms rather than one generic
//!    function — a generic `fn<T>` that mentions `TDigest` would fail to compile for `T = i64`.
//! 2. `SchemaBuilder` panics rather than returning `Result` on a duplicate or invalid
//!    registration, so every request shape is validated at the string level first (see
//!    [`validate_measure_aggregates`]) and registration assumes already-valid input.
//!
//! Dimension names and interned dimension values are not handled here: `accreta::Schema::dimensions`
//! and `accreta::Engine::resolve_dimension_value` expose them directly, so the service keeps no
//! separate copy.

use accreta::aggregate_set::{MeasureBuilder, SchemaBuilder};
use accreta::aggregates::{Count, Max, Min, Sum, TDigest};
use accreta::bucket::BucketLevel;
use accreta::measures::MeasureType;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::error::ApiError;

/// The JSON-facing value type vocabulary — mirrors `accreta::measures::MeasureType`, which this
/// service does not re-export directly since it isn't `serde`-annotated upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    F64,
    I64,
    U64,
}

impl From<MeasureType> for ValueType {
    fn from(t: MeasureType) -> Self {
        match t {
            MeasureType::F64 => ValueType::F64,
            MeasureType::I64 => ValueType::I64,
            MeasureType::U64 => ValueType::U64,
        }
    }
}

impl std::fmt::Display for ValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ValueType::F64 => "f64",
            ValueType::I64 => "i64",
            ValueType::U64 => "u64",
        };
        f.write_str(s)
    }
}

/// Every aggregate name this service knows how to register/read, i.e. every built-in
/// `accreta::aggregates::*::NAME` constant. Kept as one list so "unknown aggregate" validation
/// and the two dispatch tables below can never silently drift apart.
pub const KNOWN_AGGREGATES: &[&str] = &["sum", "count", "min", "max", "tdigest"];

/// Parse a bucket-level string (`"second"`, `"minute"`, ...) into `accreta::BucketLevel`.
///
/// Shared by `POST /schema/query`'s `level` field and `POST /schema`'s `retention` keys, so the
/// two can never silently accept different sets of level names. `field` is the dotted path to
/// use in the error envelope if `s` isn't a recognized level (e.g. `"level"` or
/// `"retention.foo"`).
pub fn parse_level(s: &str, field: impl Into<String>) -> Result<BucketLevel, ApiError> {
    match s {
        "second" => Ok(BucketLevel::Second),
        "minute" => Ok(BucketLevel::Minute),
        "hour" => Ok(BucketLevel::Hour),
        "day" => Ok(BucketLevel::Day),
        "week" => Ok(BucketLevel::Week),
        "month" => Ok(BucketLevel::Month),
        "year" => Ok(BucketLevel::Year),
        other => Err(ApiError::validation(
            "invalid_level",
            format!("unknown level '{other}'"),
            field,
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// Validation (pure string-level checks, no accreta types touched — this is what lets
// `register_measure` below assume its input is already valid and therefore never panic, since
// `SchemaBuilder` panics rather than returning `Result` on a duplicate/invalid registration).
// ---------------------------------------------------------------------------------------------

/// Validate one measure's `(value_type, aggregates)` pair against the rules in the design
/// summary: unknown aggregate names, duplicate aggregates within the measure, and `tdigest`
/// requiring `f64`. `field_prefix` is something like `"measures[1]"`, used to build the dotted
/// `field` path in the error envelope.
pub fn validate_measure_aggregates(
    value_type: ValueType,
    aggregates: &[String],
    field_prefix: &str,
) -> Result<(), ApiError> {
    let mut seen: Vec<&str> = Vec::with_capacity(aggregates.len());

    for agg in aggregates {
        let agg = agg.as_str();

        if !KNOWN_AGGREGATES.contains(&agg) {
            return Err(ApiError::unknown_aggregate(
                agg,
                format!("{field_prefix}.aggregates"),
            ));
        }

        if seen.contains(&agg) {
            return Err(ApiError::duplicate_aggregate(
                agg,
                format!("{field_prefix}.aggregates"),
            ));
        }
        seen.push(agg);

        if agg == "tdigest" && value_type != ValueType::F64 {
            return Err(ApiError::type_mismatch(
                "tdigest requires value_type f64",
                format!("{field_prefix}.aggregates"),
            ));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Schema construction — turns validated (name, value_type, aggregates) triples into calls
// against accreta's SchemaBuilder. Callers MUST run validate_measure_aggregates first: this
// function assumes its input can't trigger a panic inside SchemaBuilder.
// ---------------------------------------------------------------------------------------------

fn register_f64_aggregate(mb: &mut MeasureBuilder<'_, f64>, name: &str) {
    match name {
        "sum" => {
            mb.with::<Sum<f64>>();
        }
        "count" => {
            mb.with_any::<Count>();
        }
        "min" => {
            mb.with::<Min<f64>>();
        }
        "max" => {
            mb.with::<Max<f64>>();
        }
        "tdigest" => {
            mb.with::<TDigest>();
        }
        _ => unreachable!("validated against KNOWN_AGGREGATES"),
    }
}

fn register_i64_aggregate(mb: &mut MeasureBuilder<'_, i64>, name: &str) {
    match name {
        "sum" => {
            mb.with::<Sum<i64>>();
        }
        "count" => {
            mb.with_any::<Count>();
        }
        "min" => {
            mb.with::<Min<i64>>();
        }
        "max" => {
            mb.with::<Max<i64>>();
        }
        // Excluded by validate_measure_aggregates before this is ever reached.
        _ => unreachable!("tdigest/unknown excluded by validation"),
    }
}

fn register_u64_aggregate(mb: &mut MeasureBuilder<'_, u64>, name: &str) {
    match name {
        "sum" => {
            mb.with::<Sum<u64>>();
        }
        "count" => {
            mb.with_any::<Count>();
        }
        "min" => {
            mb.with::<Min<u64>>();
        }
        "max" => {
            mb.with::<Max<u64>>();
        }
        _ => unreachable!("tdigest/unknown excluded by validation"),
    }
}

/// Register one measure (already validated) onto `builder`.
///
/// `name` is borrowed; the schema copies it into its own `Arc<str>`, so no leaking is involved
/// and the name is freed along with the schema.
pub fn register_measure(
    builder: &mut SchemaBuilder,
    name: &str,
    value_type: ValueType,
    aggregates: &[String],
) {
    match value_type {
        ValueType::F64 => {
            let mut mb = builder.measure::<f64>(name);
            for agg in aggregates {
                register_f64_aggregate(&mut mb, agg);
            }
        }
        ValueType::I64 => {
            let mut mb = builder.measure::<i64>(name);
            for agg in aggregates {
                register_i64_aggregate(&mut mb, agg);
            }
        }
        ValueType::U64 => {
            let mut mb = builder.measure::<u64>(name);
            for agg in aggregates {
                register_u64_aggregate(&mut mb, agg);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Value extraction — the read-time mirror of the above: given a measure's declared value_type,
// pull a named aggregate's current value out of an already-merged AggregateSet as a JSON Value.
// `quantile` is only consulted for "tdigest"; ignored otherwise.
// ---------------------------------------------------------------------------------------------

fn extract_f64(
    set: &accreta::AggregateSet,
    aggregate: &str,
    quantile: Option<f64>,
) -> Option<Value> {
    match aggregate {
        "sum" => set.get::<Sum<f64>>().map(|s| Value::from(s.value())),
        "count" => set.get::<Count>().map(|c| Value::from(c.value())),
        "min" => set
            .get::<Min<f64>>()
            .map(|m| m.value().map(Value::from).unwrap_or(Value::Null)),
        "max" => set
            .get::<Max<f64>>()
            .map(|m| m.value().map(Value::from).unwrap_or(Value::Null)),
        "tdigest" => set
            .get::<TDigest>()
            .map(|d| Value::from(d.quantile(quantile.unwrap_or(0.5)))),
        _ => None,
    }
}

fn extract_i64(set: &accreta::AggregateSet, aggregate: &str) -> Option<Value> {
    match aggregate {
        "sum" => set.get::<Sum<i64>>().map(|s| Value::from(s.value())),
        "count" => set.get::<Count>().map(|c| Value::from(c.value())),
        "min" => set
            .get::<Min<i64>>()
            .map(|m| m.value().map(Value::from).unwrap_or(Value::Null)),
        "max" => set
            .get::<Max<i64>>()
            .map(|m| m.value().map(Value::from).unwrap_or(Value::Null)),
        _ => None,
    }
}

fn extract_u64(set: &accreta::AggregateSet, aggregate: &str) -> Option<Value> {
    match aggregate {
        "sum" => set.get::<Sum<u64>>().map(|s| Value::from(s.value())),
        "count" => set.get::<Count>().map(|c| Value::from(c.value())),
        "min" => set
            .get::<Min<u64>>()
            .map(|m| m.value().map(Value::from).unwrap_or(Value::Null)),
        "max" => set
            .get::<Max<u64>>()
            .map(|m| m.value().map(Value::from).unwrap_or(Value::Null)),
        _ => None,
    }
}

/// Extract `aggregate`'s current value from `set`, given the measure's declared `value_type`.
///
/// Returns `None` only if `aggregate` was never registered on this measure (a query-time
/// invariant violation, since query validation already checks the select list against the
/// schema) — callers should treat `None` as an internal-error case, not a user-facing 400.
pub fn extract_value(
    set: &accreta::AggregateSet,
    value_type: ValueType,
    aggregate: &str,
    quantile: Option<f64>,
) -> Option<Value> {
    match value_type {
        ValueType::F64 => extract_f64(set, aggregate, quantile),
        ValueType::I64 => extract_i64(set, aggregate),
        ValueType::U64 => extract_u64(set, aggregate),
    }
}
