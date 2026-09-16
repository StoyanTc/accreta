//! Unified error envelope reused across every 400/401/404/409 response.
//!
//! Every handler in this service returns `Result<T, ApiError>`. `ApiError` knows its own HTTP
//! status and serializes to the single JSON shape documented in the design summary:
//! `{"error": "<code>", "detail": "<message>", "field": "<optional dotted path>"}`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    /// Machine-readable error code, e.g. "type_mismatch", "unauthorized", "no_schema".
    pub error: String,
    /// Human-readable detail.
    pub detail: String,
    /// Optional dotted path to the offending field, e.g. "measures[1].aggregates".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub detail: String,
    pub field: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            status,
            code,
            detail: detail.into(),
            field: None,
        }
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    // ---- 400s -------------------------------------------------------------------------------

    pub fn type_mismatch(detail: impl Into<String>, field: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "type_mismatch", detail).with_field(field)
    }

    pub fn unknown_aggregate(name: &str, field: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "unknown_aggregate",
            format!("unknown aggregate '{name}'"),
        )
        .with_field(field)
    }

    pub fn duplicate_aggregate(name: &str, field: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "duplicate_aggregate",
            format!("aggregate '{name}' listed more than once for this measure"),
        )
        .with_field(field)
    }

    pub fn duplicate_name(name: &str, field: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "duplicate_name",
            format!("duplicate dimension or measure name '{name}'"),
        )
        .with_field(field)
    }

    pub fn validation(
        code: &'static str,
        detail: impl Into<String>,
        field: impl Into<String>,
    ) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, detail).with_field(field)
    }

    // ---- 401/404/409 -------------------------------------------------------------------------

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "unauthorized")
    }

    pub fn no_schema() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "no_schema",
            "no schema has been created for this tenant yet",
        )
    }

    pub fn schema_already_exists() -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "schema_already_exists",
            "a schema already exists for this tenant; it cannot be redefined in v1",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: self.code.to_string(),
            detail: self.detail,
            field: self.field,
        };
        (self.status, Json(body)).into_response()
    }
}
