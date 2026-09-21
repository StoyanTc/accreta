//! `POST /schema/ingest` — atomic per request: every sample is validated before any is applied.
//!
//! Timestamps are taken as strings (not `chrono::DateTime` directly in the request struct) so a
//! malformed one produces this service's own `{"error": "invalid_timestamp", ...,
//! "field": "samples[N].ts"}` envelope instead of a generic body-parsing 400 from serde/axum with
//! no `sample_index`.

use std::sync::Arc;

use accreta::measures::MeasureValue;
use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::auth::TenantId;
use crate::dispatch::ValueType;
use crate::error::ApiError;
use crate::state::{AppState, SchemaMeta};

#[derive(Debug, Deserialize, ToSchema)]
pub struct SampleRequest {
    pub ts: String,
    pub measures: Vec<Value>,
    pub dimensions: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct IngestRequest {
    pub samples: Vec<SampleRequest>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct IngestResponse {
    pub ingested: usize,
}

/// One validated sample, ready to hand to `Engine::ingest` and the shadow dictionaries.
struct ValidSample {
    ts: DateTime<Utc>,
    measures: Vec<MeasureValue>,
    dimensions: Vec<String>,
}

fn convert_measure(
    value: &Value,
    value_type: ValueType,
    field: &str,
) -> Result<MeasureValue, ApiError> {
    match value_type {
        ValueType::F64 => value
            .as_f64()
            .map(MeasureValue::F64)
            .ok_or_else(|| ApiError::validation("measure_type_mismatch", "expected f64", field)),
        ValueType::I64 => value
            .as_i64()
            .map(MeasureValue::I64)
            .ok_or_else(|| ApiError::validation("measure_type_mismatch", "expected i64", field)),
        ValueType::U64 => value
            .as_u64()
            .map(MeasureValue::U64)
            .ok_or_else(|| ApiError::validation("measure_type_mismatch", "expected u64", field)),
    }
}

fn validate_batch(req: &IngestRequest, meta: &SchemaMeta) -> Result<Vec<ValidSample>, ApiError> {
    let mut out = Vec::with_capacity(req.samples.len());

    for (i, sample) in req.samples.iter().enumerate() {
        let ts = DateTime::parse_from_rfc3339(&sample.ts)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| {
                ApiError::validation(
                    "invalid_timestamp",
                    format!("could not parse '{}' as an RFC3339 timestamp", sample.ts),
                    format!("samples[{i}].ts"),
                )
            })?;

        if sample.dimensions.len() != meta.dimensions.len() {
            return Err(ApiError::validation(
                "dimension_count_mismatch",
                format!(
                    "expected {} dimensions, got {}",
                    meta.dimensions.len(),
                    sample.dimensions.len()
                ),
                format!("samples[{i}].dimensions"),
            ));
        }

        if sample.measures.len() != meta.measures.len() {
            return Err(ApiError::validation(
                "measure_count_mismatch",
                format!(
                    "expected {} measures, got {}",
                    meta.measures.len(),
                    sample.measures.len()
                ),
                format!("samples[{i}].measures"),
            ));
        }

        let mut measures = Vec::with_capacity(sample.measures.len());
        for (j, (raw, def)) in sample.measures.iter().zip(meta.measures.iter()).enumerate() {
            measures.push(convert_measure(
                raw,
                def.value_type,
                &format!("samples[{i}].measures[{j}]"),
            )?);
        }

        out.push(ValidSample {
            ts,
            measures,
            dimensions: sample.dimensions.clone(),
        });
    }

    Ok(out)
}

#[utoipa::path(
    post,
    path = "/schema/ingest",
    request_body = IngestRequest,
    responses(
        (status = 200, description = "Batch ingested", body = IngestResponse),
        (status = 400, description = "Validation error", body = crate::error::ErrorBody),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorBody),
        (status = 404, description = "No schema yet", body = crate::error::ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "ingest"
)]
pub async fn ingest(
    State(state): State<Arc<AppState>>,
    tenant: TenantId,
    Json(req): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, ApiError> {
    let record = state
        .credentials
        .get(&tenant.0)
        .ok_or_else(ApiError::unauthorized)?;

    let slot = record.schema.read().await;
    let engine_state_lock = slot.as_ref().ok_or_else(ApiError::no_schema)?.clone();
    drop(slot);

    let mut engine_state = engine_state_lock.write().await;

    let valid = validate_batch(&req, &engine_state.meta)?;

    for sample in &valid {
        // Keep the shadow dictionaries in lockstep with accreta's own internal ones: same
        // strings, same per-dimension order, same call-per-sample cadence as Engine::ingest
        // below (see state.rs module docs for why this mirroring exists at all).
        for (dim_idx, value) in sample.dimensions.iter().enumerate() {
            engine_state.dim_dicts[dim_idx].get_or_insert(value);
        }

        engine_state
            .engine
            .ingest(
                sample.ts,
                sample.measures.clone(),
                sample.dimensions.clone(),
            )
            .map_err(|e| ApiError::validation("ingest_error", e.to_string(), "samples"))?;
    }

    Ok(Json(IngestResponse {
        ingested: valid.len(),
    }))
}
