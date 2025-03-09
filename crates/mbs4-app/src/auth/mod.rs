use std::time::Duration;

use crate::state::AppState;
use axum::{
    extract::{FromRequest as _, State},
    response::{IntoResponse, Redirect},
    routing::get,
    Extension, Form, Json,
};
use http::StatusCode;
use time::OffsetDateTime;
use tower_cookies::Cookies;
use tower_sessions::Session;
use tracing::{debug, error, warn};

const SESSION_COOKIE_NAME: &str = "mbs4";
const TOKEN_COOKIE_NAME: &str = "mbs4_token";
const SESSION_USER_KEY: &str = "user";
const SESSION_EXPIRY_SECS: u64 = 3600;

pub mod oidc;
pub mod token;

pub async fn logout(
    session: Session,
    state: State<AppState>,
    cookies: Cookies,
) -> Result<impl IntoResponse, StatusCode> {
    let redirect_url = state.build_url("/").map_err(|e| {
        error!("Failed to build redirect URL: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    session
        .delete()
        .await
        .unwrap_or_else(|e| warn!("Failed to delete session: {e}"));

    cookies.remove(tower_cookies::Cookie::new(SESSION_COOKIE_NAME, ""));

    Ok(Redirect::temporary(redirect_url.as_str()))
}

/// Builds authentication router - must be nested on /auth path!
pub fn auth_router() -> axum::Router<AppState> {
    let session_store = tower_sessions::MemoryStore::default();
    let session_layer = tower_sessions::SessionManagerLayer::new(session_store)
        .with_name(SESSION_COOKIE_NAME)
        .with_secure(true)
        .with_expiry(tower_sessions::Expiry::AtDateTime(
            OffsetDateTime::now_utc() + Duration::from_secs(SESSION_EXPIRY_SECS),
        ));
    axum::Router::new()
        .route("/login", get(oidc::login).post(db_login))
        .route("/callback", get(oidc::callback))
        .route("/logout", get(logout))
        .route("/token", get(token::token))
        .layer(session_layer)
        .layer(Extension(oidc::ProvidersCache::new()))
}

#[derive(serde::Deserialize)]
struct LoginCredentials {
    email: String,
    password: String,
}

pub async fn after_ok_login(
    state: &AppState,
    session: &Session,
    known_user: mbs4_dal::user::User,
) -> Result<impl IntoResponse, StatusCode> {
    session
        .insert(SESSION_USER_KEY, known_user)
        .await
        .map_err(|e| {
            error!("Failed to store user in session: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let redirect_url = state.build_url("/").map_err(|e| {
        error!("Failed to build redirect URL: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Redirect::temporary(redirect_url.as_str()))
}

pub async fn db_login(
    state: State<AppState>,
    user_registry: mbs4_dal::user::UserRepository,
    session: Session,
    request: axum::extract::Request,
) -> Result<impl IntoResponse, StatusCode> {
    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let credentials = if content_type == "application/json" {
        let Json(data) = Json::<LoginCredentials>::from_request(request, &())
            .await
            .map_err(|e| {
                error!("Failed to get login credentials: {e}");
                StatusCode::BAD_REQUEST
            })?;
        data
    } else if content_type == "application/x-www-form-urlencoded" {
        let Form(data) = axum::extract::Form::<LoginCredentials>::from_request(request, &())
            .await
            .map_err(|e| {
                error!("Failed to get login credentials: {e}");
                StatusCode::BAD_REQUEST
            })?;
        data
    } else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let user = user_registry
        .check_password(&credentials.email, &credentials.password)
        .await
        .map_err(|e| {
            debug!("User check error: {e}");
            StatusCode::UNAUTHORIZED
        })?;

    after_ok_login(&state, &session, user).await
}
