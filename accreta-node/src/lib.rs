//! Node.js bindings for `accreta`, generated with napi-rs.
//!
//! This binds directly to the `accreta` Rust crate (not through `accreta-ffi`'s C ABI) so there
//! is no extra marshaling layer between JS and Rust. As with `accreta-ffi`, only the fixed set of
//! built-in aggregates (`sum`, `count`, `min`, `max`, `average`) is exposed — custom/generic
//! aggregate state is not reachable from JS, since a JS caller can't supply a Rust type at
//! compile time.
//!
//! ## Design notes for future maintainers
//!
//! - **Dimension/measure names must be `&'static str`** in the underlying `accreta` API
//!   (`SchemaBuilder::dimension`, `SchemaBuilder::measure`). Since JS gives us owned, runtime
//!   `String`s, we `Box::leak` them once at schema-build time. Schemas are normally built once at
//!   startup and live for the process lifetime, so this is a small, one-time, intentional leak —
//!   not a per-request leak. Don't call `Engine::new` in a hot loop.
//! - **`accreta::Engine` does not expose its internal dimension dictionaries.** There is no public
//!   way to resolve a `DimensionValueId` back to the original string through the crate's public
//!   API. To work around this, `Engine` (this wrapper) keeps its own mirrored dictionaries,
//!   updated in lock-step with every `ingest()` call using the exact same first-seen-gets-next-id
//!   scheme as `accreta`'s internal `DimensionDictionary`. Because both dictionaries are built
//!   from the same calls in the same order, the ids always agree. If `accreta` ever grows a public
//!   accessor for its dictionaries, this mirroring can be deleted in favor of that.
//! - **Measure values cross the boundary as `f64`** (JS has no distinct integer type). For `i64`/
//!   `u64` measures we cast on the way in and out. This is fine for realistic counts/sums; values
//!   near or beyond 2^53 will lose precision, same as any other JS number.

use std::collections::HashMap;

use napi::bindgen_prelude::*;
use napi_derive::napi;

use accreta::aggregates::{Average, Count, Max, Min, Sum};
use accreta::measures::{MeasureId, MeasureType, MeasureValue};
use accreta::{BucketLevel, DimensionId, DimensionMask};

// ---------------------------------------------------------------------------
// JS-facing input/output shapes
// ---------------------------------------------------------------------------

/// One measure to register on the schema.
#[napi(object)]
pub struct MeasureSpec {
    pub name: String,
    /// One of `"f64"`, `"i64"`, `"u64"`.
    pub value_type: String,
    /// Subset of `"sum"`, `"count"`, `"min"`, `"max"`, `"average"`.
    pub aggregates: Vec<String>,
}

/// A retention window for one bucket level, in milliseconds.
#[napi(object)]
pub struct RetentionSpec {
    /// One of `"minute"`, `"hour"`, `"day"`, `"week"`, `"month"`, `"year"`.
    pub level: String,
    pub max_age_ms: f64,
}

/// The full schema for an `Engine`: dimensions, measures, and (optionally) retention.
#[napi(object)]
pub struct SchemaSpec {
    pub dimensions: Vec<String>,
    pub measures: Vec<MeasureSpec>,
    pub retention: Option<Vec<RetentionSpec>>,
}

/// The subset of built-in aggregates that were actually registered for a measure.
/// Fields that weren't registered (or have no data yet, for min/max/average) are `null`.
#[napi(object)]
pub struct AggregateResult {
    pub sum: Option<f64>,
    pub count: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub average: Option<f64>,
}

#[napi(object)]
pub struct GroupResult {
    /// Dimension values for this group, in schema registration order (i.e. one entry per
    /// dimension passed to the `SchemaSpec`, not just the ones you queried by).
    pub dimension_values: Vec<String>,
    /// One `AggregateResult` per measure, in schema registration order.
    pub measures: Vec<AggregateResult>,
}

#[napi(object)]
pub struct BucketResult {
    pub level: String,
    pub start_ms: f64,
    pub end_ms: f64,
    pub groups: Vec<GroupResult>,
}

#[napi(object)]
pub struct GroupedAggregateResult {
    /// The dimension names this result is grouped by, in ascending schema-registration order
    /// (which may differ from the order you passed to `queryRangeGrouped`).
    pub dimensions: Vec<String>,
    /// Values parallel to `dimensions`.
    pub dimension_values: Vec<String>,
    pub aggregate: AggregateResult,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

#[napi]
pub struct Engine {
    inner: accreta::Engine,
    dimension_names: Vec<String>,
    measure_names: Vec<String>,
    // Mirrored dimension dictionaries — see module docs above for why these exist.
    dict_names: Vec<Vec<String>>,
    dict_lookup: Vec<HashMap<String, u32>>,
}

#[napi]
impl Engine {
    #[napi(constructor)]
    pub fn new(spec: SchemaSpec) -> Result<Self> {
        if spec.dimensions.is_empty() {
            return Err(Error::from_reason(
                "schema must define at least one dimension",
            ));
        }
        if spec.measures.is_empty() {
            return Err(Error::from_reason("schema must define at least one measure"));
        }

        let mut builder = accreta::Schema::builder();

        for dim in &spec.dimensions {
            // Intentional one-time leak — see module docs.
            let leaked: &'static str = Box::leak(dim.clone().into_boxed_str());
            builder.dimension(leaked);
        }

        for measure in &spec.measures {
            let value_type = parse_value_type(&measure.value_type)?;
            let leaked: &'static str = Box::leak(measure.name.clone().into_boxed_str());
            register_measure(&mut builder, leaked, value_type, &measure.aggregates)?;
        }

        let schema = builder.build().map_err(to_napi_err)?;
        let retention = build_retention(&spec.retention)?;
        let inner = accreta::Engine::with_retention(schema, retention);

        let dimension_names = spec.dimensions.clone();
        let measure_names = spec.measures.iter().map(|m| m.name.clone()).collect();
        let dict_names = vec![Vec::new(); dimension_names.len()];
        let dict_lookup = vec![HashMap::new(); dimension_names.len()];

        Ok(Self {
            inner,
            dimension_names,
            measure_names,
            dict_names,
            dict_lookup,
        })
    }

    #[napi(getter)]
    pub fn dimension_names(&self) -> Vec<String> {
        self.dimension_names.clone()
    }

    #[napi(getter)]
    pub fn measure_names(&self) -> Vec<String> {
        self.measure_names.clone()
    }

    /// Fold one sample into the appropriate minute bucket. `timestampMs` is milliseconds since
    /// the Unix epoch (e.g. `Date.now()`). `measures` and `dimensions` must match the schema's
    /// registration order and length.
    #[napi]
    pub fn ingest(
        &mut self,
        timestamp_ms: f64,
        measures: Vec<f64>,
        dimensions: Vec<String>,
    ) -> Result<()> {
        if dimensions.len() != self.dimension_names.len() {
            return Err(Error::from_reason(format!(
                "expected {} dimension values, got {}",
                self.dimension_names.len(),
                dimensions.len()
            )));
        }
        if measures.len() != self.measure_names.len() {
            return Err(Error::from_reason(format!(
                "expected {} measure values, got {}",
                self.measure_names.len(),
                measures.len()
            )));
        }

        // Mirror the dictionary update *before* handing off to the real engine, using the same
        // first-seen-gets-next-id scheme, so ids stay in lockstep.
        for (idx, value) in dimensions.iter().enumerate() {
            if !self.dict_lookup[idx].contains_key(value) {
                let id = self.dict_names[idx].len() as u32;
                self.dict_lookup[idx].insert(value.clone(), id);
                self.dict_names[idx].push(value.clone());
            }
        }

        let schema = self.inner.schema().clone();
        let mut measure_values = Vec::with_capacity(measures.len());
        for (idx, value) in measures.iter().enumerate() {
            let data_type = schema
                .measure(MeasureId(idx as u8))
                .expect("measure count validated above")
                .data_type;
            measure_values.push(to_measure_value(*value, data_type));
        }

        let timestamp = ms_to_datetime(timestamp_ms)?;

        self.inner
            .ingest(timestamp, measure_values, dimensions.iter())
            .map_err(to_napi_err)
    }

    /// Recompute every level above `minute` by merging bucket states upward. Safe to call
    /// repeatedly — it never re-reads raw samples, only merges existing bucket states.
    #[napi]
    pub fn rollup(&mut self) {
        self.inner.rollup();
    }

    /// Discard buckets older than the configured retention window for their level. A no-op for
    /// any level with no retention configured.
    #[napi]
    pub fn prune(&mut self) {
        self.inner.prune();
    }

    #[napi]
    pub fn bucket_count(&self, level: String) -> Result<u32> {
        let level = parse_level(&level)?;
        Ok(self.inner.bucket_count(level) as u32)
    }

    /// All buckets currently stored at `level`, each with every dimension group's aggregate
    /// state resolved back to plain numbers/strings.
    #[napi]
    pub fn buckets(&self, level: String) -> Result<Vec<BucketResult>> {
        let level = parse_level(&level)?;
        let schema = self.inner.schema();
        let level_name = level.to_string();

        let mut out = Vec::new();
        for bucket in self.inner.buckets(level) {
            let mut groups = Vec::new();
            for (key, sets) in bucket.groups() {
                let dimension_values = self.resolve_full_key(key.values());

                let mut measures = Vec::with_capacity(sets.len());
                for (idx, set) in sets.iter().enumerate() {
                    let data_type = schema
                        .measure(MeasureId(idx as u8))
                        .expect("bucket measure sets are aligned with schema")
                        .data_type;
                    measures.push(read_aggregates(set, data_type));
                }

                groups.push(GroupResult {
                    dimension_values,
                    measures,
                });
            }

            out.push(BucketResult {
                level: level_name.clone(),
                start_ms: bucket.start().timestamp_millis() as f64,
                end_ms: bucket.end().timestamp_millis() as f64,
                groups,
            });
        }

        Ok(out)
    }

    /// Merge every bucket overlapping `[startMs, endMs)` at `level` into one total, across all
    /// dimension groups. For a per-dimension breakdown use `queryRangeGrouped`.
    #[napi]
    pub fn query_range(
        &self,
        level: String,
        start_ms: f64,
        end_ms: f64,
        measure_index: u32,
    ) -> Result<AggregateResult> {
        let level = parse_level(&level)?;
        let start = ms_to_datetime(start_ms)?;
        let end = ms_to_datetime(end_ms)?;
        let measure = MeasureId(measure_index as u8);

        let data_type = self
            .inner
            .schema()
            .measure(measure)
            .ok_or_else(|| Error::from_reason(format!("invalid measure index {measure_index}")))?
            .data_type;

        let set = self
            .inner
            .query_range(level, start, end, measure)
            .map_err(to_napi_err)?;

        Ok(read_aggregates(&set, data_type))
    }

    /// Like `queryRange`, but grouped by the dimensions named in `groupBy`. An empty `groupBy`
    /// returns a single row (the grand total), equivalent to `queryRange`.
    #[napi]
    pub fn query_range_grouped(
        &self,
        level: String,
        start_ms: f64,
        end_ms: f64,
        measure_index: u32,
        group_by: Vec<String>,
    ) -> Result<Vec<GroupedAggregateResult>> {
        let level = parse_level(&level)?;
        let start = ms_to_datetime(start_ms)?;
        let end = ms_to_datetime(end_ms)?;
        let measure = MeasureId(measure_index as u8);

        let data_type = self
            .inner
            .schema()
            .measure(measure)
            .ok_or_else(|| Error::from_reason(format!("invalid measure index {measure_index}")))?
            .data_type;

        let mut indices = Vec::with_capacity(group_by.len());
        for name in &group_by {
            let idx = self
                .dimension_names
                .iter()
                .position(|d| d == name)
                .ok_or_else(|| Error::from_reason(format!("unknown dimension '{name}'")))?;
            indices.push(idx);
        }
        indices.sort_unstable();
        indices.dedup();

        let mut mask = DimensionMask::new();
        for idx in &indices {
            mask = mask.with(DimensionId(*idx as u8));
        }

        let grouped = self
            .inner
            .query_range_grouped(level, start, end, measure, mask)
            .map_err(to_napi_err)?;

        let dimensions: Vec<String> = indices
            .iter()
            .map(|i| self.dimension_names[*i].clone())
            .collect();

        let mut out = Vec::with_capacity(grouped.len());
        for (key, set) in grouped {
            let dimension_values: Vec<String> = key
                .values()
                .iter()
                .zip(&indices)
                .map(|(id, dim_idx)| self.resolve_one(*dim_idx, *id))
                .collect();

            out.push(GroupedAggregateResult {
                dimensions: dimensions.clone(),
                dimension_values,
                aggregate: read_aggregates(&set, data_type),
            });
        }

        Ok(out)
    }

    fn resolve_full_key(&self, values: &[u32]) -> Vec<String> {
        values
            .iter()
            .enumerate()
            .map(|(idx, id)| self.resolve_one(idx, *id))
            .collect()
    }

    fn resolve_one(&self, dimension_index: usize, value_id: u32) -> String {
        self.dict_names[dimension_index]
            .get(value_id as usize)
            .cloned()
            .unwrap_or_else(|| format!("<unknown:{value_id}>"))
    }
}

// ---------------------------------------------------------------------------
// Schema construction helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum ValueType {
    F64,
    I64,
    U64,
}

fn parse_value_type(s: &str) -> Result<ValueType> {
    match s {
        "f64" => Ok(ValueType::F64),
        "i64" => Ok(ValueType::I64),
        "u64" => Ok(ValueType::U64),
        other => Err(Error::from_reason(format!(
            "unknown value type '{other}', expected 'f64', 'i64', or 'u64'"
        ))),
    }
}

fn parse_level(level: &str) -> Result<BucketLevel> {
    match level.to_ascii_lowercase().as_str() {
        "minute" => Ok(BucketLevel::Minute),
        "hour" => Ok(BucketLevel::Hour),
        "day" => Ok(BucketLevel::Day),
        "week" => Ok(BucketLevel::Week),
        "month" => Ok(BucketLevel::Month),
        "year" => Ok(BucketLevel::Year),
        other => Err(Error::from_reason(format!(
            "unknown bucket level '{other}', expected one of: minute, hour, day, week, month, year"
        ))),
    }
}

/// Registers `name` on `builder` with the built-in aggregates named in `aggregates`.
///
/// Only the fixed set of built-in aggregates is reachable from JS (same scope decision as
/// `accreta-ffi`) — custom/generic aggregate state needs a Rust type at compile time, which a JS
/// caller can't supply.
fn register_measure(
    builder: &mut accreta::SchemaBuilder,
    name: &'static str,
    value_type: ValueType,
    aggregates: &[String],
) -> Result<()> {
    macro_rules! register_all {
        ($mb:expr, $t:ty) => {
            for agg in aggregates {
                match agg.as_str() {
                    "sum" => {
                        $mb.with::<Sum<$t>>();
                    }
                    "count" => {
                        $mb.with_any::<Count>();
                    }
                    "min" => {
                        $mb.with::<Min<$t>>();
                    }
                    "max" => {
                        $mb.with::<Max<$t>>();
                    }
                    "average" => {
                        $mb.with::<Average<$t>>();
                    }
                    other => {
                        return Err(Error::from_reason(format!(
                            "unknown aggregate '{other}', expected one of: sum, count, min, max, average"
                        )));
                    }
                }
            }
        };
    }

    match value_type {
        ValueType::F64 => {
            let mut mb = builder.measure::<f64>(name);
            register_all!(mb, f64);
        }
        ValueType::I64 => {
            let mut mb = builder.measure::<i64>(name);
            register_all!(mb, i64);
        }
        ValueType::U64 => {
            let mut mb = builder.measure::<u64>(name);
            register_all!(mb, u64);
        }
    }

    Ok(())
}

fn build_retention(spec: &Option<Vec<RetentionSpec>>) -> Result<accreta::Retention> {
    let mut retention = accreta::Retention::new();
    if let Some(entries) = spec {
        for entry in entries {
            let level = parse_level(&entry.level)?;
            let max_age = chrono::Duration::milliseconds(entry.max_age_ms as i64);
            retention = retention.keep(level, max_age);
        }
    }
    Ok(retention)
}

// ---------------------------------------------------------------------------
// Value conversion helpers
// ---------------------------------------------------------------------------

fn to_measure_value(value: f64, data_type: MeasureType) -> MeasureValue {
    match data_type {
        MeasureType::F64 => MeasureValue::F64(value),
        MeasureType::I64 => MeasureValue::I64(value as i64),
        MeasureType::U64 => MeasureValue::U64(value as u64),
    }
}

fn ms_to_datetime(ms: f64) -> Result<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .ok_or_else(|| Error::from_reason(format!("invalid timestamp (ms): {ms}")))
}

fn read_aggregates(set: &accreta::AggregateSet, data_type: MeasureType) -> AggregateResult {
    // Count has no type parameter, so this is shared across all three branches below.
    let count = set.get::<Count>().map(|c| c.value() as f64);

    macro_rules! read_typed {
        ($t:ty, $cast:expr) => {{
            let cast: fn($t) -> f64 = $cast;
            AggregateResult {
                sum: set.get::<Sum<$t>>().map(|s| cast(s.value())),
                count,
                min: set.get::<Min<$t>>().and_then(|m| m.value()).map(cast),
                max: set.get::<Max<$t>>().and_then(|m| m.value()).map(cast),
                average: set.get::<Average<$t>>().and_then(|a| {
                    if a.count() == 0 {
                        None
                    } else {
                        Some(cast(a.sum()) / a.count() as f64)
                    }
                }),
            }
        }};
    }

    match data_type {
        MeasureType::F64 => read_typed!(f64, |v: f64| v),
        MeasureType::I64 => read_typed!(i64, |v: i64| v as f64),
        MeasureType::U64 => read_typed!(u64, |v: u64| v as f64),
    }
}

fn to_napi_err<E: std::fmt::Display>(e: E) -> Error {
    Error::from_reason(e.to_string())
}
