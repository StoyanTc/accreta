//! `#[derive(OpenApi)]` root: wires every handler's `#[utoipa::path]` into one spec, and declares
//! the Bearer HTTP security scheme used by everything except `POST /login`.

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::health::health,
        crate::auth::login,
        crate::schema_api::create_schema,
        crate::schema_api::get_schema,
        crate::ingest_api::ingest,
        crate::query_api::query,
    ),
    components(schemas(
        crate::health::HealthResponse,
        crate::auth::LoginRequest,
        crate::auth::LoginResponse,
        crate::schema_api::MeasureRequest,
        crate::schema_api::SchemaRequest,
        crate::schema_api::MeasureResponse,
        crate::schema_api::SchemaResponse,
        crate::ingest_api::SampleRequest,
        crate::ingest_api::IngestRequest,
        crate::ingest_api::IngestResponse,
        crate::query_api::TimeRange,
        crate::query_api::SelectItem,
        crate::query_api::QueryRequest,
        crate::query_api::GroupResult,
        crate::query_api::BucketResult,
        crate::query_api::QueryResponse,
        crate::dispatch::ValueType,
        crate::error::ErrorBody,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Liveness and rollup-freshness probe"),
        (name = "auth", description = "Login and token issuance"),
        (name = "schema", description = "Schema creation and introspection"),
        (name = "ingest", description = "Sample ingestion"),
        (name = "query", description = "Rollup queries"),
    ),
    info(
        title = "accreta-metrics",
        description = "Reference/demo service exposing the accreta mergeable-state aggregation engine over HTTP.",
        version = "0.1.0",
    )
)]
pub struct ApiDoc;
