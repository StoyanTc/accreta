//! Node.js bindings for `accreta`, generated with napi-rs.
//!
//! This binds directly to the `accreta` Rust crate (not through `accreta-ffi`'s C ABI) so there
//! is no extra marshaling layer between JS and Rust. As with `accreta-ffi`, only the fixed set of
//! built-in aggregates (`sum`, `count`, `min`, `max`, `average`, `tdigest`) is exposed —
//! custom/generic aggregate state is not reachable from JS, since a JS caller can't supply a Rust
//! type at compile time.
//!
//! ## Design notes for future maintainers
//!
//! - **`tdigest` is exposed as an opaque `TDigestHandle` class, not a plain number.** Unlike the
//!   other aggregates, a quantile estimate needs a `q` parameter supplied at query time, so it
//!   can't be flattened into an `Option<f64>` field the way `sum`/`min`/`max`/`average` are.
//!   `TDigestHandle::quantile(q)` lets JS ask for any quantile on demand instead of us guessing
//!   which percentiles the caller wants ahead of time.
//! - **Shadow measures for cross-type `tdigest`.** `accreta::SchemaBuilder::with` requires
//!   `Aggregator::Input == T`, and `TDigest::Input` is a fixed `f64` (intentionally not generic
//!   over the measure's numeric type — see `accreta::aggregates::TDigest`'s own docs), so it can
//!   only attach directly to an already-`f64` measure. For an `i64`/`u64` measure that requests
//!   `"tdigest"`, `Engine::new` registers a second, internal-only `f64` measure (named
//!   `__tdigest_shadow__<name>`, never exposed via `measureNames`) with `TDigest` on *that*, and
//!   `ingest` fans each sample out to both the real measure and its shadow, cast to `f64`.
//!   `tdigest_shadow_id` tracks the real→shadow `MeasureId` mapping; `queryRangeTDigest` resolves
//!   through it, and `buckets`/schema-facing measure counts stay truncated to the real measures
//!   so shadows are invisible from JS. This is deliberately a wrapper-only mechanism — `accreta`
//!   itself never sees anything unusual, just two ordinary independently-typed measures written
//!   together — so it doesn't require, and doesn't presume, any change to `accreta` core or to
//!   `accreta-py`/`accreta-ffi`. Cost: one extra measure's worth of bucket storage, across the
//!   full retention hierarchy, per `i64`/`u64` measure that uses `tdigest`.
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

use accreta::aggregates::{Average, Count, Max, Min, Sum, TDigest};
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
    /// Subset of `"sum"`, `"count"`, `"min"`, `"max"`, `"average"`, `"tdigest"`. `"tdigest"`
    /// works for any `valueType` — see the module docs' note on shadow measures for how `i64`/
    /// `u64` measures get there.
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

/// Opaque handle onto a `TDigest`'s compressed state, letting JS query any quantile on demand
/// instead of a fixed set of percentiles chosen ahead of time on the Rust side.
///
/// `quantile()` returns `f64::NAN` on an empty digest (no samples folded in yet) — same as the
/// underlying `accreta::aggregates::TDigest::quantile`. Callers that need to distinguish "empty"
/// from "a genuine NaN can't happen here" should track that on the JS side (e.g. by checking
/// whether they've ingested anything for this group/measure yet).
#[napi]
pub struct TDigestHandle {
    inner: TDigest,
}

#[napi]
impl TDigestHandle {
    /// Estimate the value at quantile `q` (`0.0..=1.0`).
    #[napi]
    pub fn quantile(&self, q: f64) -> f64 {
        self.inner.quantile(q)
    }
}

/// The subset of built-in aggregates that were actually registered for a measure.
/// Fields that weren't registered (or have no data yet, for min/max/average) are `null`.
///
/// `tdigest` is deliberately *not* a field here — `TDigestHandle` is a `#[napi]` class, and class
/// instances can't be embedded in a `#[napi(object)]` struct (the object macro derives
/// `FromNapiValue` for every field, which class instances don't implement by value). Use
/// `Engine::queryRangeTDigest` to fetch a `TDigestHandle` for a measure directly.
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
    // `tdigest_shadow_id[i] = Some(shadow_id)` if measure `i` requested `tdigest` while not
    // being `f64`-typed — see the module docs' "Shadow measures for cross-type tdigest" note.
    // `None` covers both "no tdigest requested" and "tdigest registered directly (f64 measure)".
    tdigest_shadow_id: Vec<Option<u8>>,
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
            return Err(Error::from_reason(
                "schema must define at least one measure",
            ));
        }

        let mut builder = accreta::Schema::builder();

        for dim in &spec.dimensions {
            // Intentional one-time leak — see module docs.
            let leaked: &'static str = Box::leak(dim.clone().into_boxed_str());
            builder.dimension(leaked);
        }

        let mut needs_shadow = Vec::with_capacity(spec.measures.len());
        for measure in &spec.measures {
            let value_type = parse_value_type(&measure.value_type)?;
            let leaked: &'static str = Box::leak(measure.name.clone().into_boxed_str());
            let shadow = register_measure(&mut builder, leaked, value_type, &measure.aggregates)?;
            needs_shadow.push(shadow);
        }

        // Shadow tdigest measures are registered *after* every real measure, in a second pass —
        // never interleaved with the loop above — so real measures keep MeasureIds 0..N exactly
        // matching spec.measures' order, which the rest of this file (ingest, buckets,
        // query_range, ...) already assumes. Each shadow gets the next id after that, in the
        // order its owning real measure appears in spec.measures. This relies on
        // SchemaBuilder::measure assigning ids sequentially by call order, starting at 0 — the
        // same assumption the rest of this file already makes for the real measures.
        let mut tdigest_shadow_id = vec![None; spec.measures.len()];
        let mut next_shadow_id = spec.measures.len() as u8;
        for (idx, needs) in needs_shadow.iter().enumerate() {
            if *needs {
                let shadow_name: &'static str = Box::leak(
                    format!("__tdigest_shadow__{}", spec.measures[idx].name).into_boxed_str(),
                );
                let mut smb = builder.measure::<f64>(shadow_name);
                smb.with::<TDigest>();
                tdigest_shadow_id[idx] = Some(next_shadow_id);
                next_shadow_id += 1;
            }
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
            tdigest_shadow_id,
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
        // Fan each shadowed value out to its tdigest shadow measure too, cast to f64. This must
        // come after the loop above (real measures first) and in idx order, matching how shadow
        // measures were registered in Engine::new — see tdigest_shadow_id's doc comment.
        for (idx, value) in measures.iter().enumerate() {
            if self.tdigest_shadow_id[idx].is_some() {
                measure_values.push(MeasureValue::F64(*value));
            }
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

                let mut measures = Vec::with_capacity(self.measure_names.len());
                // .take(...) drops any shadow tdigest measures appended after the real ones —
                // see tdigest_shadow_id's doc comment. Without this, `measures` here would have
                // more entries than `measure_names`, breaking positional zipping on the JS side.
                for (idx, set) in sets.iter().take(self.measure_names.len()).enumerate() {
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

    /// Like `queryRange`, but for the `tdigest` aggregate specifically — returns a handle you can
    /// call `.quantile(q)` on for any `q`, rather than a fixed set of precomputed percentiles.
    /// `null` if `tdigest` wasn't registered on this measure, or if the range has no samples yet.
    ///
    /// Kept as its own method rather than a field on `AggregateResult` because `TDigestHandle` is
    /// a class instance and can't be embedded in a `#[napi(object)]` result — see the doc comment
    /// on `AggregateResult`.
    #[napi]
    pub fn query_range_tdigest(
        &self,
        level: String,
        start_ms: f64,
        end_ms: f64,
        measure_index: u32,
    ) -> Result<Option<TDigestHandle>> {
        let level = parse_level(&level)?;
        let start = ms_to_datetime(start_ms)?;
        let end = ms_to_datetime(end_ms)?;

        let idx = measure_index as usize;
        let Some(shadow) = self.tdigest_shadow_id.get(idx) else {
            return Err(Error::from_reason(format!(
                "measure index {idx} out of range (schema has {} measures)",
                self.measure_names.len()
            )));
        };
        // If this measure requested tdigest on a non-f64 type, it lives on the shadow f64
        // measure instead — see tdigest_shadow_id's doc comment. Otherwise (f64 measure with
        // tdigest, or no tdigest requested at all) query the real measure id directly; in the
        // "not requested" case `set.get::<TDigest>()` below just correctly returns `None`.
        let query_id = match shadow {
            Some(shadow_id) => MeasureId(*shadow_id),
            None => MeasureId(measure_index as u8),
        };

        let set = self
            .inner
            .query_range(level, start, end, query_id)
            .map_err(to_napi_err)?;

        Ok(set
            .get::<TDigest>()
            .cloned()
            .map(|inner| TDigestHandle { inner }))
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
///
/// Returns `Ok(true)` if `aggregates` requested `"tdigest"` on a non-`f64` measure, meaning the
/// caller (`Engine::new`) still needs to register a shadow `f64` measure for it — see that
/// function's doc comment for why. Returns `Ok(false)` if no shadow is needed, either because
/// `"tdigest"` wasn't requested at all, or because `value_type` is already `F64` (in which case
/// `tdigest` is registered directly on `name`, right here, with no shadow required).
fn register_measure(
    builder: &mut accreta::SchemaBuilder,
    name: &'static str,
    value_type: ValueType,
    aggregates: &[String],
) -> Result<bool> {
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
                    "tdigest" => {
                        // Deliberately not registered here — see the per-ValueType branches
                        // below. TDigest::Input is a fixed f64 (see its module docs), and
                        // SchemaBuilder::with requires A::Input == T exactly, so `$mb.with::<
                        // TDigest>()` only type-checks in the f64 instantiation of this macro.
                        // Writing it here unconditionally would fail to compile for the i64/u64
                        // instantiations even though this arm never runs for them at runtime —
                        // Rust still type-checks macro-generated code for every value of $t it's
                        // invoked with. This arm's only job is to keep "tdigest" recognized as a
                        // valid name instead of falling through to the `other` error case below.
                    }
                    other => {
                        return Err(Error::from_reason(format!(
                            "unknown aggregate '{other}', expected one of: sum, count, min, max, average, tdigest"
                        )));
                    }
                }
            }
        };
    }

    let wants_tdigest = aggregates.iter().any(|a| a == "tdigest");

    match value_type {
        ValueType::F64 => {
            let mut mb = builder.measure::<f64>(name);
            register_all!(mb, f64);
            if wants_tdigest {
                // T == f64 == TDigest::Input already, so plain `with` works — no shadow needed.
                mb.with::<TDigest>();
            }
            Ok(false)
        }
        ValueType::I64 => {
            let mut mb = builder.measure::<i64>(name);
            register_all!(mb, i64);
            Ok(wants_tdigest)
        }
        ValueType::U64 => {
            let mut mb = builder.measure::<u64>(name);
            register_all!(mb, u64);
            Ok(wants_tdigest)
        }
    }
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
