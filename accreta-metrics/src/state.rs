//! Application state: the seeded credential store and, once created, each tenant's `accreta`
//! engine.
//!
//! v1 is 1:1:1 (one tenant, one user, one schema), so `DashMap<Username, TenantRecord>` doubles
//! as the tenant registry — there is no separate schema registry.
//!
//! ## Why this also carries a shadow dimension dictionary
//!
//! Reading `accreta` 0.1.1's source turned up two gaps versus what the design summary assumed:
//!
//! - `accreta::Schema` exposes `dimension_count()` but never the dimension **names** — only
//!   `Schema::measures()` is introspectable that way. So this service has to track dimension
//!   names itself (`SchemaMeta::dimensions` below), in the same order they were declared —
//!   which is also `DimensionId` order, since `SchemaBuilder::dimension` assigns IDs
//!   sequentially.
//! - `accreta::Engine` interns dimension *values* (e.g. `"us-east"`) into per-dimension
//!   dictionaries at ingest time, but never exposes a way to read them back — no
//!   `resolve(id) -> &str` and no filter-by-value query. `Engine::query_range_grouped` only
//!   groups by a `DimensionMask`; it has no equality-filter parameter at all.
//!
//! Both gaps are closed the same way: this service keeps its own [`DimDict`] per dimension,
//! updated in lockstep with every `Engine::ingest` call (same strings, same per-sample order),
//! which gives it both directions (`resolve` for building query responses, and `value -> id` for
//! implementing the `filter` clause by hand over `Engine::buckets`/`Bucket::groups` — see
//! `query_api.rs`).

use std::sync::Arc;
use std::{collections::HashMap, sync::atomic::AtomicI64};

use accreta::engine::Engine;
use dashmap::DashMap;
use jsonwebtoken::{DecodingKey, EncodingKey};

use crate::dispatch::ValueType;

/// One dimension's value <-> id mapping, mirroring `accreta::dimensions::DimensionDictionary`
/// (which is private to the `accreta` crate) closely enough to stay in sync with it: both assign
/// ids in first-seen order, starting at 0, and this service is the only caller of
/// `Engine::ingest`, so the two dictionaries can never drift apart.
#[derive(Debug, Default)]
pub struct DimDict {
    forward: HashMap<String, u32>,
    reverse: Vec<String>,
}

impl DimDict {
    /// Insert `value` if new, returning its id either way. Mirrors
    /// `DimensionDictionary::get_or_insert`'s "next free index" allocation exactly.
    pub fn get_or_insert(&mut self, value: &str) -> u32 {
        if let Some(&id) = self.forward.get(value) {
            return id;
        }
        let id = self.reverse.len() as u32;
        self.forward.insert(value.to_owned(), id);
        self.reverse.push(value.to_owned());
        id
    }

    /// Look up an id without inserting — used to translate a query's `filter` values into the
    /// ids stored in bucket keys. A value never ingested simply can't match anything.
    pub fn get(&self, value: &str) -> Option<u32> {
        self.forward.get(value).copied()
    }

    /// Resolve an id back to its string, for building query response `dimensions` arrays.
    pub fn resolve(&self, id: u32) -> Option<&str> {
        self.reverse.get(id as usize).map(String::as_str)
    }
}

/// Metadata this service tracks alongside `accreta::Schema` to compensate for what `Schema`
/// itself doesn't expose (dimension names — see module docs).
#[derive(Debug, Clone)]
pub struct MeasureMeta {
    pub name: String,
    pub value_type: ValueType,
    pub aggregates: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SchemaMeta {
    pub name: String,
    /// Declaration order == `DimensionId` order.
    pub dimensions: Vec<String>,
    /// Declaration order == `MeasureId` order.
    pub measures: Vec<MeasureMeta>,
}

/// Everything that exists once a tenant has called `POST /schema`: the live `accreta::Engine`,
/// this service's own metadata mirror, and the shadow dictionaries.
///
/// All three change together on every ingest, so they share one lock rather than three —
/// avoiding a window where the engine has a sample the shadow dictionary doesn't yet know about.
pub struct EngineState {
    pub engine: Engine,
    pub meta: SchemaMeta,
    /// One `DimDict` per dimension, indexed by `DimensionId`.
    pub dim_dicts: Vec<DimDict>,
}

impl EngineState {
    pub fn new(engine: Engine, meta: SchemaMeta) -> Self {
        let dim_dicts = (0..meta.dimensions.len())
            .map(|_| DimDict::default())
            .collect();
        Self {
            engine,
            meta,
            dim_dicts,
        }
    }
}

/// One tenant's record in the seeded credential store.
pub struct TenantRecord {
    pub password_hash: String,
    /// The `sub` claim issued in this tenant's JWTs.
    pub tenant_id: String,
    /// `None` until `POST /schema` succeeds once. The outer lock guards the one-shot
    /// create-vs-already-exists decision (write-locked only for the duration of that check);
    /// the inner lock guards ordinary ingest/query traffic against the live engine.
    pub schema: tokio::sync::RwLock<Option<Arc<tokio::sync::RwLock<EngineState>>>>,
}

pub struct JwtKeys {
    pub encoding: EncodingKey,
    pub decoding: DecodingKey,
}

pub struct AppState {
    pub jwt: JwtKeys,
    /// Username -> tenant record. Seeded once at startup from config; there is no `/signup` in
    /// v1 (see design summary).
    pub credentials: DashMap<String, TenantRecord>,
    /// Unix millis of the last fully-completed rollup+prune sweep; 0 until the first tick.
    /// `AtomicI64` rather than an `RwLock<Option<DateTime<Utc>>>` so `/health` stays cheap and
    /// lock-free regardless of what ingest/query/rollup are doing.
    pub last_rollup_sweep: AtomicI64,
}
