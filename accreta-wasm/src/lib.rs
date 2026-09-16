//! WebAssembly bindings for `accreta`, generated with `wasm-bindgen`.
//!
//! This binds directly to the `accreta` Rust crate (not through `accreta-ffi`'s C ABI), the same
//! scope decision as `accreta-node`: only the fixed set of built-in aggregates (`sum`, `count`,
//! `min`, `max`, `average`, `tdigest`) is exposed — custom/generic aggregate state is not
//! reachable from JS, since a JS caller can't supply a Rust type at compile time.
//!
//! ## Differences from `accreta-node`, and why
//!
//! - **Dimension/measure names are interned, not unconditionally leaked.** `accreta-node` calls
//!   `Box::leak` on every name on every `Engine::new`, with no dedup — safe there under the
//!   documented assumption that schemas are built once at process startup. That assumption is
//!   less safe in a browser tab, which can live for hours/days and may rebuild an `Engine` on
//!   every hot-reload cycle. `intern()` below leaks a given string at most once per WASM
//!   instance: re-registering the same name (e.g. an identical schema rebuilt on hot-reload)
//!   reuses the existing leak instead of allocating a fresh one. This bounds leaks by *distinct
//!   names ever seen*, not by *`Engine`s created* — if a caller genuinely registers many distinct
//!   names over time (not the same schema repeated), this doesn't help, and nothing here reclaims
//!   memory when an `Engine` is dropped. Doing that properly would mean threading a non-`'static`
//!   lifetime through `accreta::Schema`/`Engine` themselves — a change to accreta core, not this
//!   wrapper. See project notes for the fuller discussion of that tradeoff.
//! - **`thread_local!` + `RefCell`, not `Mutex`/`Lazy`.** `wasm32-unknown-unknown` is
//!   single-threaded unless the threads proposal is explicitly enabled (SharedArrayBuffer +
//!   cross-origin isolation), so there's no need for the locking a Node addon might reach for.
//! - **Complex return shapes go through `serde` + `serde-wasm-bindgen`, not typed structs.**
//!   napi-rs's `#[napi(object)]` auto-marshals nested `Vec<CustomStruct>` into JS objects/arrays.
//!   `wasm-bindgen` has no equivalent for nested struct vectors, so `buckets()`, `query_range()`,
//!   and `query_range_grouped()` return `JsValue` (built via `serde_wasm_bindgen::to_value`)
//!   instead of a typed result. `Engine` and `TDigestHandle` stay as real `#[wasm_bindgen]`
//!   classes, same as their napi-rs counterparts — they're opaque handles, not plain data.
//! - **Errors are `JsError`, not `napi::Error`.** `wasm_bindgen::JsError` implements
//!   `From<E: Display>`, so `Result<T, JsError>` works directly without a `to_napi_err`-style
//!   helper; exported functions surface these as real JS exceptions.
//! - **The `tdigest` compression default is still config-file-backed in accreta core, which does
//!   not work in a browser (no filesystem).** This wrapper does not fix that — it's a change to
//!   accreta core (moving the default to a `build.rs`-generated compile-time constant, plus
//!   ideally an explicit per-measure override). Left as a TODO; `mb.with::<TDigest>()` below will
//!   panic or fail at whatever point accreta core's current file read happens, until that's
//!   addressed upstream.
//!
//! Everything else — the shadow-measure mechanism for `tdigest` on `i64`/`u64` measures, the
//! mirrored dimension dictionaries (since `accreta::Engine` doesn't expose its internal ones),
//! and measure values crossing as `f64` — is unchanged from `accreta-node` and carries the same
//! caveats (see the shadow-measure and precision notes inline below).

use std::cell::RefCell;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use accreta::aggregates::{Count, Max, Min, Sum, TDigest};
use accreta::measures::{MeasureId, MeasureType, MeasureValue};
use accreta::{BucketLevel, DimensionId, DimensionMask};

// ---------------------------------------------------------------------------
// Name interning
// ---------------------------------------------------------------------------

thread_local! {
    // Global to this WASM instance (not per-Engine), so re-registering the same name across
    // multiple `Engine::new` calls reuses the existing leak. See module docs above.
    static INTERNED: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
}

/// Returns a `&'static str` for `s`, leaking a new allocation only the first time this exact
/// string is seen in this WASM instance. Bounds leaks by distinct names ever seen, not by
/// `Engine`s created — see module docs for the limits of this approach.
fn intern(s: &str) -> &'static str {
    INTERNED.with(|table| {
        let mut table = table.borrow_mut();
        if let Some(existing) = table.get(s) {
            return *existing;
        }
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        table.insert(s.to_string(), leaked);
        leaked
    })
}

// ---------------------------------------------------------------------------
// JS-facing input/output shapes
// ---------------------------------------------------------------------------

/// One measure to register on the schema.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasureSpec {
    pub name: String,
    /// One of `"f64"`, `"i64"`, `"u64"`.
    pub value_type: String,
    /// Subset of `"sum"`, `"count"`, `"min"`, `"max"`, `"average"`, `"tdigest"`. `"tdigest"`
    /// works for any `valueType` — see the module docs' note on shadow measures.
    pub aggregates: Vec<String>,
}

/// A retention window for one bucket level, in milliseconds.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionSpec {
    /// One of `"minute"`, `"hour"`, `"day"`, `"week"`, `"month"`, `"year"`.
    pub level: String,
    pub max_age_ms: f64,
}

/// The full schema for an `Engine`: dimensions, measures, and (optionally) retention.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSpec {
    pub dimensions: Vec<String>,
    pub measures: Vec<MeasureSpec>,
    pub retention: Option<Vec<RetentionSpec>>,
}

/// The subset of built-in aggregates that were actually registered for a measure. Fields that
/// weren't registered (or have no data yet, for min/max) are `null` on the JS side.
///
/// `tdigest` is deliberately not a field here, for the same reason as `accreta-node`:
/// `TDigestHandle` is a real class instance, not plain data, so it doesn't round-trip through
/// `serde_wasm_bindgen`. Use `Engine::query_range_tdigest` to fetch one directly.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateResult {
    pub sum: Option<f64>,
    pub count: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupResult {
    /// Dimension values for this group, in schema registration order.
    pub dimension_values: Vec<String>,
    /// One `AggregateResult` per measure, in schema registration order.
    pub measures: Vec<AggregateResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketResult {
    pub level: String,
    pub start_ms: f64,
    pub end_ms: f64,
    pub groups: Vec<GroupResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupedAggregateResult {
    /// Dimension names this result is grouped by, in ascending schema-registration order.
    pub dimensions: Vec<String>,
    /// Values parallel to `dimensions`.
    pub dimension_values: Vec<String>,
    pub aggregate: AggregateResult,
}

/// Opaque handle onto a `TDigest`'s compressed state, letting JS query any quantile on demand
/// instead of a fixed set of percentiles chosen ahead of time on the Rust side.
///
/// `quantile()` returns `f64::NAN` on an empty digest — same as
/// `accreta::aggregates::TDigest::quantile`. Callers that need to distinguish "empty" from "a
/// genuine NaN can't happen here" should track that on the JS side.
#[wasm_bindgen]
pub struct TDigestHandle {
    inner: TDigest,
}

#[wasm_bindgen]
impl TDigestHandle {
    /// Estimate the value at quantile `q` (`0.0..=1.0`).
    #[wasm_bindgen]
    pub fn quantile(&self, q: f64) -> f64 {
        self.inner.quantile(q)
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct Engine {
    inner: accreta::Engine,
    dimension_names: Vec<String>,
    measure_names: Vec<String>,
    // Mirrored dimension dictionaries — see module docs above for why these exist.
    dict_names: Vec<Vec<String>>,
    dict_lookup: Vec<HashMap<String, u32>>,
    // `tdigest_shadow_id[i] = Some(shadow_id)` if measure `i` requested `tdigest` while not
    // being `f64`-typed. `None` covers both "no tdigest requested" and "tdigest registered
    // directly (f64 measure)".
    tdigest_shadow_id: Vec<Option<u8>>,
}

#[wasm_bindgen]
impl Engine {
    /// `spec` is a JS object matching `SchemaSpec` (camelCase field names: `dimensions`,
    /// `measures`, `retention`).
    #[wasm_bindgen(constructor)]
    pub fn new(spec: JsValue) -> Result<Engine, JsError> {
        let spec: SchemaSpec = serde_wasm_bindgen::from_value(spec)
            .map_err(|e| JsError::new(&format!("invalid schema spec: {e}")))?;

        if spec.dimensions.is_empty() {
            return Err(JsError::new("schema must define at least one dimension"));
        }
        if spec.measures.is_empty() {
            return Err(JsError::new("schema must define at least one measure"));
        }

        let mut builder = accreta::Schema::builder();

        for dim in &spec.dimensions {
            let leaked: &'static str = intern(dim);
            builder.dimension(leaked);
        }

        let mut needs_shadow = Vec::with_capacity(spec.measures.len());
        for measure in &spec.measures {
            let value_type = parse_value_type(&measure.value_type)?;
            let leaked: &'static str = intern(&measure.name);
            let shadow = register_measure(&mut builder, leaked, value_type, &measure.aggregates)?;
            needs_shadow.push(shadow);
        }

        // Shadow tdigest measures are registered *after* every real measure, in a second pass,
        // so real measures keep MeasureIds 0..N exactly matching spec.measures' order. Each
        // shadow gets the next id after that, in the order its owning real measure appears in
        // spec.measures. Relies on SchemaBuilder::measure assigning ids sequentially by call
        // order, starting at 0 — same assumption the rest of this file makes for real measures.
        let mut tdigest_shadow_id = vec![None; spec.measures.len()];
        let mut next_shadow_id = spec.measures.len() as u8;
        for (idx, needs) in needs_shadow.iter().enumerate() {
            if *needs {
                let shadow_name: &'static str =
                    intern(&format!("__tdigest_shadow__{}", spec.measures[idx].name));
                let mut smb = builder.measure::<f64>(shadow_name);
                smb.with::<TDigest>();
                tdigest_shadow_id[idx] = Some(next_shadow_id);
                next_shadow_id += 1;
            }
        }

        let schema = builder
            .build()
            .map_err(|e| JsError::new(&e.to_string()))?;
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

    #[wasm_bindgen(getter, js_name = dimensionNames)]
    pub fn dimension_names(&self) -> Vec<String> {
        self.dimension_names.clone()
    }

    #[wasm_bindgen(getter, js_name = measureNames)]
    pub fn measure_names(&self) -> Vec<String> {
        self.measure_names.clone()
    }

    /// Fold one sample into the appropriate minute bucket. `timestamp_ms` is milliseconds since
    /// the Unix epoch (e.g. `Date.now()`). `measures` and `dimensions` must match the schema's
    /// registration order and length.
    #[wasm_bindgen]
    pub fn ingest(
        &mut self,
        timestamp_ms: f64,
        measures: Vec<f64>,
        dimensions: Vec<String>,
    ) -> Result<(), JsError> {
        if dimensions.len() != self.dimension_names.len() {
            return Err(JsError::new(&format!(
                "expected {} dimension values, got {}",
                self.dimension_names.len(),
                dimensions.len()
            )));
        }
        if measures.len() != self.measure_names.len() {
            return Err(JsError::new(&format!(
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
        // Fan each shadowed value out to its tdigest shadow measure too, cast to f64 — must come
        // after real measures, in idx order, matching registration order in `new`.
        for (idx, value) in measures.iter().enumerate() {
            if self.tdigest_shadow_id[idx].is_some() {
                measure_values.push(MeasureValue::F64(*value));
            }
        }

        let timestamp = ms_to_datetime(timestamp_ms)?;

        self.inner
            .ingest(timestamp, measure_values, dimensions.iter())
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Recompute every level above `minute` by merging bucket states upward. Safe to call
    /// repeatedly — it never re-reads raw samples, only merges existing bucket states.
    #[wasm_bindgen]
    pub fn rollup(&mut self) {
        self.inner.rollup();
    }

    /// Discard buckets older than the configured retention window for their level. A no-op for
    /// any level with no retention configured.
    #[wasm_bindgen]
    pub fn prune(&mut self) {
        self.inner.prune();
    }

    #[wasm_bindgen(js_name = bucketCount)]
    pub fn bucket_count(&self, level: String) -> Result<u32, JsError> {
        let level = parse_level(&level)?;
        Ok(self.inner.bucket_count(level) as u32)
    }

    /// All buckets currently stored at `level`, each with every dimension group's aggregate
    /// state resolved back to plain numbers/strings. Returns a JS array of objects shaped like
    /// `BucketResult` (see the struct docs) — see module docs for why this isn't a typed return.
    #[wasm_bindgen]
    pub fn buckets(&self, level: String) -> Result<JsValue, JsError> {
        let level = parse_level(&level)?;
        let schema = self.inner.schema();
        let level_name = level.to_string();

        let mut out = Vec::new();
        for bucket in self.inner.buckets(level) {
            let mut groups = Vec::new();
            for (key, sets) in bucket.groups() {
                let dimension_values = self.resolve_full_key(key.values());

                let mut measures = Vec::with_capacity(self.measure_names.len());
                // .take(...) drops any shadow tdigest measures appended after the real ones.
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

        serde_wasm_bindgen::to_value(&out).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Merge every bucket overlapping `[start_ms, end_ms)` at `level` into one total, across all
    /// dimension groups. For a per-dimension breakdown use `query_range_grouped`. Returns a JS
    /// object shaped like `AggregateResult`.
    #[wasm_bindgen(js_name = queryRange)]
    pub fn query_range(
        &self,
        level: String,
        start_ms: f64,
        end_ms: f64,
        measure_index: u32,
    ) -> Result<JsValue, JsError> {
        let level = parse_level(&level)?;
        let start = ms_to_datetime(start_ms)?;
        let end = ms_to_datetime(end_ms)?;
        let measure = MeasureId(measure_index as u8);

        let data_type = self
            .inner
            .schema()
            .measure(measure)
            .ok_or_else(|| JsError::new(&format!("invalid measure index {measure_index}")))?
            .data_type;

        let set = self
            .inner
            .query_range(level, start, end, measure)
            .map_err(|e| JsError::new(&e.to_string()))?;

        let result = read_aggregates(&set, data_type);
        serde_wasm_bindgen::to_value(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Like `query_range`, but for the `tdigest` aggregate specifically — returns a handle you
    /// can call `.quantile(q)` on for any `q`, rather than a fixed set of precomputed
    /// percentiles. `undefined` if `tdigest` wasn't registered on this measure, or if the range
    /// has no samples yet.
    #[wasm_bindgen(js_name = queryRangeTdigest)]
    pub fn query_range_tdigest(
        &self,
        level: String,
        start_ms: f64,
        end_ms: f64,
        measure_index: u32,
    ) -> Result<Option<TDigestHandle>, JsError> {
        let level = parse_level(&level)?;
        let start = ms_to_datetime(start_ms)?;
        let end = ms_to_datetime(end_ms)?;

        let idx = measure_index as usize;
        let Some(shadow) = self.tdigest_shadow_id.get(idx) else {
            return Err(JsError::new(&format!(
                "measure index {idx} out of range (schema has {} measures)",
                self.measure_names.len()
            )));
        };
        // If this measure requested tdigest on a non-f64 type, it lives on the shadow f64
        // measure instead. Otherwise, query the real measure id directly; in the "not requested"
        // case `set.get::<TDigest>()` below correctly returns `None`.
        let query_id = match shadow {
            Some(shadow_id) => MeasureId(*shadow_id),
            None => MeasureId(measure_index as u8),
        };

        let set = self
            .inner
            .query_range(level, start, end, query_id)
            .map_err(|e| JsError::new(&e.to_string()))?;

        Ok(set
            .get::<TDigest>()
            .cloned()
            .map(|inner| TDigestHandle { inner }))
    }

    /// Like `query_range`, but grouped by the dimensions named in `group_by`. An empty
    /// `group_by` returns a single row (the grand total). Returns a JS array of objects shaped
    /// like `GroupedAggregateResult`.
    #[wasm_bindgen(js_name = queryRangeGrouped)]
    pub fn query_range_grouped(
        &self,
        level: String,
        start_ms: f64,
        end_ms: f64,
        measure_index: u32,
        group_by: Vec<String>,
    ) -> Result<JsValue, JsError> {
        let level = parse_level(&level)?;
        let start = ms_to_datetime(start_ms)?;
        let end = ms_to_datetime(end_ms)?;
        let measure = MeasureId(measure_index as u8);

        let data_type = self
            .inner
            .schema()
            .measure(measure)
            .ok_or_else(|| JsError::new(&format!("invalid measure index {measure_index}")))?
            .data_type;

        let mut indices = Vec::with_capacity(group_by.len());
        for name in &group_by {
            let idx = self
                .dimension_names
                .iter()
                .position(|d| d == name)
                .ok_or_else(|| JsError::new(&format!("unknown dimension '{name}'")))?;
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
            .map_err(|e| JsError::new(&e.to_string()))?;

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

        serde_wasm_bindgen::to_value(&out).map_err(|e| JsError::new(&e.to_string()))
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

fn parse_value_type(s: &str) -> Result<ValueType, JsError> {
    match s {
        "f64" => Ok(ValueType::F64),
        "i64" => Ok(ValueType::I64),
        "u64" => Ok(ValueType::U64),
        other => Err(JsError::new(&format!(
            "unknown value type '{other}', expected 'f64', 'i64', or 'u64'"
        ))),
    }
}

fn parse_level(level: &str) -> Result<BucketLevel, JsError> {
    match level.to_ascii_lowercase().as_str() {
        "minute" => Ok(BucketLevel::Minute),
        "hour" => Ok(BucketLevel::Hour),
        "day" => Ok(BucketLevel::Day),
        "week" => Ok(BucketLevel::Week),
        "month" => Ok(BucketLevel::Month),
        "year" => Ok(BucketLevel::Year),
        other => Err(JsError::new(&format!(
            "unknown bucket level '{other}', expected one of: minute, hour, day, week, month, year"
        ))),
    }
}

/// Registers `name` on `builder` with the built-in aggregates named in `aggregates`.
///
/// Returns `Ok(true)` if `aggregates` requested `"tdigest"` on a non-`f64` measure, meaning the
/// caller (`Engine::new`) still needs to register a shadow `f64` measure for it. Returns
/// `Ok(false)` if no shadow is needed, either because `"tdigest"` wasn't requested at all, or
/// because `value_type` is already `F64` (registered directly on `name`, no shadow required).
fn register_measure(
    builder: &mut accreta::SchemaBuilder,
    name: &'static str,
    value_type: ValueType,
    aggregates: &[String],
) -> Result<bool, JsError> {
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
                    "tdigest" => {
                        // Deliberately not registered here — see the per-ValueType branches
                        // below. TDigest::Input is a fixed f64, and SchemaBuilder::with requires
                        // A::Input == T exactly, so this only type-checks in the f64
                        // instantiation. This arm's only job is to keep "tdigest" recognized as
                        // valid instead of falling through to the `other` error case.
                    }
                    other => {
                        return Err(JsError::new(&format!(
                            "unknown aggregate '{other}', expected one of: sum, count, min, max, tdigest"
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
                // NOTE: this call is where accreta core's config-file-backed compression default
                // is currently read — see module docs' TODO on why that breaks in a browser.
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

fn build_retention(spec: &Option<Vec<RetentionSpec>>) -> Result<accreta::Retention, JsError> {
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

fn ms_to_datetime(ms: f64) -> Result<chrono::DateTime<chrono::Utc>, JsError> {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .ok_or_else(|| JsError::new(&format!("invalid timestamp (ms): {ms}")))
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
            }
        }};
    }

    match data_type {
        MeasureType::F64 => read_typed!(f64, |v: f64| v),
        MeasureType::I64 => read_typed!(i64, |v: i64| v as f64),
        MeasureType::U64 => read_typed!(u64, |v: u64| v as f64),
    }
}
