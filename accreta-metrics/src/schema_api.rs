//! `POST /schema` (create, one-shot) and `GET /schema` (introspect).

use std::collections::HashMap;
use std::sync::Arc;

use accreta::engine::Engine;
use accreta::retention::Retention;
use axum::Json;
use axum::extract::State;
use chrono::Duration;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::TenantId;
use crate::dispatch::{self, ValueType};
use crate::error::ApiError;
use crate::state::{AppState, EngineState, MeasureMeta, SchemaMeta};

#[derive(Debug, Deserialize, ToSchema)]
pub struct MeasureRequest {
    pub name: String,
    pub value_type: ValueType,
    pub aggregates: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SchemaRequest {
    pub name: String,
    pub dimensions: Vec<String>,
    pub measures: Vec<MeasureRequest>,
    /// Optional per-level retention, keyed by level name (`"second"`, `"minute"`, `"hour"`,
    /// `"day"`, `"week"`, `"month"`, `"year"`) with a max-age value in seconds. A level not
    /// present here is kept forever (`accreta`'s default) — this is the only way to configure
    /// retention in v1; there's no endpoint to change it after schema creation. Safe to set on
    /// any level: `accreta`'s incremental rollup means a pruned bucket's contribution is already
    /// durably folded into its parent before it's discarded.
    #[serde(default)]
    pub retention: HashMap<String, u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MeasureResponse {
    pub name: String,
    pub value_type: ValueType,
    pub aggregates: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SchemaResponse {
    pub name: String,
    pub dimensions: Vec<String>,
    pub measures: Vec<MeasureResponse>,
    /// Echoes back whatever was configured in the request's `retention` field, if anything.
    pub retention: HashMap<String, u64>,
}

/// Validate the whole request shape: unknown/duplicate aggregates and tdigest-on-non-f64 (via
/// [`dispatch::validate_measure_aggregates`]), duplicate dimension/measure names across the
/// schema as a whole, and the `retention` map's level names and values. Returns nothing on
/// success — `accreta::Schema::builder()` and `accreta::Retention` are only ever called with
/// input that has already passed every check here, so neither can panic or reject it.
fn validate_schema_request(req: &SchemaRequest) -> Result<(), ApiError> {
    if req.dimensions.is_empty() {
        return Err(ApiError::validation(
            "no_dimensions",
            "schema must define at least one dimension",
            "dimensions",
        ));
    }
    if req.measures.is_empty() {
        return Err(ApiError::validation(
            "no_measures",
            "schema must define at least one measure",
            "measures",
        ));
    }

    let mut names: Vec<&str> = Vec::new();
    for (i, d) in req.dimensions.iter().enumerate() {
        if names.contains(&d.as_str()) {
            return Err(ApiError::duplicate_name(d, format!("dimensions[{i}]")));
        }
        names.push(d.as_str());
    }
    for (i, m) in req.measures.iter().enumerate() {
        if names.contains(&m.name.as_str()) {
            return Err(ApiError::duplicate_name(
                &m.name,
                format!("measures[{i}].name"),
            ));
        }
        names.push(m.name.as_str());

        dispatch::validate_measure_aggregates(
            m.value_type,
            &m.aggregates,
            &format!("measures[{i}]"),
        )?;
    }

    for (level_str, seconds) in &req.retention {
        dispatch::parse_level(level_str, format!("retention.{level_str}"))?;
        if *seconds == 0 {
            return Err(ApiError::validation(
                "invalid_retention",
                "retention max_age_seconds must be greater than 0",
                format!("retention.{level_str}"),
            ));
        }
    }

    Ok(())
}

fn build_engine_state(req: SchemaRequest) -> EngineState {
    let mut builder = accreta::Schema::builder();

    for d in &req.dimensions {
        builder.dimension(dispatch::leak(d));
    }

    let mut measure_meta = Vec::with_capacity(req.measures.len());
    for m in &req.measures {
        let leaked_name = dispatch::leak(&m.name);
        dispatch::register_measure(&mut builder, leaked_name, m.value_type, &m.aggregates);
        measure_meta.push(MeasureMeta {
            name: m.name.clone(),
            value_type: m.value_type,
            aggregates: m.aggregates.clone(),
        });
    }

    // Validated up front in validate_schema_request — build() only fails on
    // NoDimensions/NoMeasures, both already checked there, so this can't realistically fail.
    let schema = builder
        .build()
        .expect("schema request was already validated to have >=1 dimension and >=1 measure");

    // Retention level names were already validated in validate_schema_request, so parse_level
    // here can't fail — accreta's incremental rollup (see accreta's engine.rs docs) means this is
    // safe for any level, not just leaf levels like Week/Year.
    let mut retention = Retention::new();
    for (level_str, seconds) in &req.retention {
        let level = dispatch::parse_level(level_str, "retention")
            .expect("retention keys were validated before build_engine_state was called");
        retention = retention.keep(level, Duration::seconds(*seconds as i64));
    }

    let meta = SchemaMeta {
        name: req.name,
        dimensions: req.dimensions,
        measures: measure_meta,
        retention: req.retention,
    };

    EngineState::new(Engine::with_retention(schema, retention), meta)
}

#[utoipa::path(
    post,
    path = "/schema",
    request_body = SchemaRequest,
    responses(
        (status = 201, description = "Schema created", body = SchemaResponse),
        (status = 400, description = "Validation error", body = crate::error::ErrorBody),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorBody),
        (status = 409, description = "Schema already exists", body = crate::error::ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "schema"
)]
pub async fn create_schema(
    State(state): State<Arc<AppState>>,
    tenant: TenantId,
    Json(req): Json<SchemaRequest>,
) -> Result<(axum::http::StatusCode, Json<SchemaResponse>), ApiError> {
    validate_schema_request(&req)?;

    let record = state
        .credentials
        .get(&tenant.0)
        .ok_or_else(ApiError::unauthorized)?;

    // Atomic check-and-set: hold the write lock for the whole check-then-set so two concurrent
    // POST /schema calls can't both observe "empty" and both succeed.
    let mut slot = record.schema.write().await;
    if slot.is_some() {
        return Err(ApiError::schema_already_exists());
    }

    let engine_state = build_engine_state(req);
    let response = schema_response(&engine_state.meta);
    *slot = Some(Arc::new(tokio::sync::RwLock::new(engine_state)));

    Ok((axum::http::StatusCode::CREATED, Json(response)))
}

#[utoipa::path(
    get,
    path = "/schema",
    responses(
        (status = 200, description = "Current schema", body = SchemaResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorBody),
        (status = 404, description = "No schema yet", body = crate::error::ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "schema"
)]
pub async fn get_schema(
    State(state): State<Arc<AppState>>,
    tenant: TenantId,
) -> Result<Json<SchemaResponse>, ApiError> {
    let record = state
        .credentials
        .get(&tenant.0)
        .ok_or_else(ApiError::unauthorized)?;

    let slot = record.schema.read().await;
    let engine_state = slot.as_ref().ok_or_else(ApiError::no_schema)?;
    let engine_state = engine_state.read().await;

    Ok(Json(schema_response(&engine_state.meta)))
}

fn schema_response(meta: &SchemaMeta) -> SchemaResponse {
    SchemaResponse {
        name: meta.name.clone(),
        dimensions: meta.dimensions.clone(),
        measures: meta
            .measures
            .iter()
            .map(|m| MeasureResponse {
                name: m.name.clone(),
                value_type: m.value_type,
                aggregates: m.aggregates.clone(),
            })
            .collect(),
        retention: meta.retention.clone(),
    }
}
