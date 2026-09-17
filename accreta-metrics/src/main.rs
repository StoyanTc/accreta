mod auth;
mod dispatch;
mod error;
mod health;
mod ingest_api;
mod openapi;
mod query_api;
mod rollup;
mod schema_api;
mod state;

use std::sync::Arc;

use argon2::{Argon2, PasswordHasher};
use axum::routing::post;
use axum::{Router, routing::get};
use dashmap::DashMap;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::state::{AppState, TenantRecord};

/// Seeded from config (env vars) at startup, per the design summary: v1 has no `/signup`, just
/// one demo credential baked in at process start. Falls back to a fixed demo/demo123 pair when
/// unset, since this is a reference/demo service, not a production deployment.
fn seed_credentials() -> DashMap<String, TenantRecord> {
    let username = std::env::var("ACCRETA_METRICS_USERNAME").unwrap_or_else(|_| "demo".to_string());
    let password =
        std::env::var("ACCRETA_METRICS_PASSWORD").unwrap_or_else(|_| "demo123".to_string());

    let password_hash = Argon2::default()
        .hash_password(password.as_bytes())
        .expect("hashing the seeded demo password should never fail")
        .to_string();

    let credentials = DashMap::new();
    credentials.insert(
        username.clone(),
        TenantRecord {
            password_hash,
            tenant_id: username.clone(),
            schema: tokio::sync::RwLock::new(None),
        },
    );

    tracing::info!(%username, "seeded demo credential");
    credentials
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let state = Arc::new(AppState {
        jwt: auth::new_jwt_keys(),
        credentials: seed_credentials(),
        last_rollup_sweep: std::sync::atomic::AtomicI64::new(0),
    });

    rollup::spawn(state.clone());

    let app = Router::new()
        .route("/health", get(health::health))
        .route("/login", post(auth::login))
        .route(
            "/schema",
            post(schema_api::create_schema).get(schema_api::get_schema),
        )
        .route("/schema/ingest", post(ingest_api::ingest))
        .route("/schema/query", post(query_api::query))
        .merge(
            SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi::ApiDoc::openapi()),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = std::env::var("ACCRETA_METRICS_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));

    tracing::info!(%addr, "accreta-metrics listening");
    tracing::info!("Swagger UI at http://{addr}/swagger-ui");

    axum::serve(listener, app).await.expect("server error");
}
