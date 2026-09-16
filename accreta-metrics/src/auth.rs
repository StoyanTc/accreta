//! `POST /login` and the `TenantId` extractor.
//!
//! Auth failures of every kind (missing header, malformed token, expired token, bad signature,
//! unknown user, wrong password) all collapse to one `401 {"error": "unauthorized"}` — no
//! differentiation exposed to the client, per the design summary.

use std::sync::Arc;

use argon2::{Argon2, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::Json;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ApiError;
use crate::state::AppState;

const TOKEN_TTL_MINUTES: i64 = 30;

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    /// Tenant id.
    sub: String,
    exp: i64,
}

/// `POST /login` — the only endpoint that doesn't require a bearer token.
#[utoipa::path(
    post,
    path = "/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login succeeded", body = LoginResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorBody),
    ),
    tag = "auth"
)]
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let record = state
        .credentials
        .get(&req.username)
        .ok_or_else(ApiError::unauthorized)?;

    Argon2::default()
        .verify_password(req.password.as_bytes(), record.password_hash.as_str())
        .map_err(|_| ApiError::unauthorized())?;

    let expires_at = Utc::now() + Duration::minutes(TOKEN_TTL_MINUTES);
    let claims = Claims {
        sub: record.tenant_id.clone(),
        exp: expires_at.timestamp(),
    };

    let token = encode(&Header::new(Algorithm::HS256), &claims, &state.jwt.encoding)
        .map_err(|_| ApiError::unauthorized())?;

    Ok(Json(LoginResponse { token, expires_at }))
}

/// Extracted from a valid bearer token; handlers that need auth just take this as an argument.
#[derive(Debug, Clone)]
pub struct TenantId(pub String);

impl FromRequestParts<Arc<AppState>> for TenantId {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(ApiError::unauthorized)?;

        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(ApiError::unauthorized)?;

        let data = decode::<Claims>(
            token,
            &state.jwt.decoding,
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|_| ApiError::unauthorized())?;

        Ok(TenantId(data.claims.sub))
    }
}

/// Build the HS256 signing/verification keypair once at process startup. A restart invalidates
/// every outstanding token, by design (see design summary).
///
/// Uses the `getrandom` crate directly (added as a direct dependency) rather than routing
/// through `argon2`'s re-exports: `password-hash` 0.6's `rand_core` dependency moved to
/// `rand_core` 0.10, which dropped `OsRng` entirely — `getrandom::fill` is what `password-hash`
/// itself now uses internally for this exact purpose (see `phc::Salt::generate`).
pub fn new_jwt_keys() -> crate::state::JwtKeys {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).expect("system RNG failure while generating the JWT signing key");
    crate::state::JwtKeys {
        encoding: EncodingKey::from_secret(&secret),
        decoding: DecodingKey::from_secret(&secret),
    }
}
