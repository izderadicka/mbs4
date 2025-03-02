use std::time::Duration;

use crate::state::AppState;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect},
    routing::get,
    Extension,
};
use http::StatusCode;
use time::OffsetDateTime;
use tower_cookies::Cookies;
use tower_sessions::Session;
use tracing::{error, warn};

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
        .route("/login", get(oidc::login))
        .route("/callback", get(oidc::callback))
        .route("/logout", get(logout))
        .route("/token", get(token::token))
        .layer(session_layer)
        .layer(Extension(oidc::ProvidersCache::new()))
}
