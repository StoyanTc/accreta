//! `GET /health` — unauthenticated liveness/status probe.
//!
//! Deliberately outside the JWT/TenantId extractor: the Svelte GUI needs to reach this even
//! before anyone has logged in, and a load balancer / uptime probe can't carry a bearer token.
//! It reports only shape-of-state, never schema/dimension/measure content, so exposing it
//! unauthenticated doesn't leak tenant data — even under the current 1:1:1 model where there's
//! only one tenant to leak, and where the demo credentials are already public via Swagger UI.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Always "ok" if the process can answer at all — this endpoint never returns non-200.
    pub status: &'static str,
    /// True once some tenant has called `POST /schema` successfully. Lets the client skip an
    /// authenticated `GET /schema` (which 404s pre-schema anyway) just to decide whether to show
    /// a "seed demo data" prompt — even pre-login.
    pub schema_present: bool,
    /// When the background rollup+prune sweep last completed a full pass, if ever. `None` before
    /// the first tick. Lets the GUI render "data as of {this + one bucket duration}" instead of
    /// inferring freshness from a query response's `bucket_start` values.
    pub last_rollup_sweep: Option<DateTime<Utc>>,
}

async fn schema_present(state: &AppState) -> bool {
    for entry in state.credentials.iter() {
        if entry.value().schema.read().await.is_some() {
            return true;
        }
    }
    false
}

fn last_rollup_sweep(state: &AppState) -> Option<DateTime<Utc>> {
    match state.last_rollup_sweep.load(Ordering::Relaxed) {
        0 => None,
        millis => DateTime::from_timestamp_millis(millis),
    }
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is up", body = HealthResponse),
    ),
    tag = "health"
)]
pub async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        schema_present: schema_present(&state).await,
        last_rollup_sweep: last_rollup_sweep(&state),
    })
}
