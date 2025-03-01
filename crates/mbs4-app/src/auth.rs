use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, RwLock},
    time::Duration,
};

use crate::{
    dal::user::{User, UserRepository},
    state::AppState,
};
use axum::{
    extract::{FromRequestParts, Query, State},
    response::{IntoResponse, Redirect},
    routing::get,
    Extension, RequestPartsExt,
};
use cookie::{Cookie, Expiration, SameSite};
use http::StatusCode;
use mbs4_types::claim::{ApiClaim, UserClaim};
use serde::Deserialize;
use time::OffsetDateTime;
use tower_cookies::Cookies;
use tower_sessions::Session;
use tracing::{debug, error, warn};

use mbs4_auth::oidc::{OIDCClient, OIDCSecrets};

const SESSION_COOKIE_NAME: &str = "mbs4";
const TOKEN_COOKIE_NAME: &str = "mbs4_token";
const SESSION_SECRETS_KEY: &str = "oidc_secrets";
const SESSION_PROVIDER_KEY: &str = "oidc_provider";
const SESSION_USER_KEY: &str = "user";
const SESSION_EXPIRY_SECS: u64 = 3600;

#[derive(Debug, Deserialize)]
pub struct LoginParams {
    oidc_provider: String,
}

impl FromRequestParts<AppState> for OIDCClient {
    type Rejection = StatusCode;

    fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &AppState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> {
        async {
            let query = Query::<LoginParams>::from_request_parts(parts, state).await;
            let session = parts.extract::<Session>().await.map_err(|e| {
                error!("Missing session for OIDC provider: {}", e.1);
                e.0
            })?;

            let provider_id = match query {
                Ok(params) => {
                    let params = params.0;
                    session
                        .insert(SESSION_PROVIDER_KEY, params.oidc_provider.clone())
                        .await
                        .map_err(|e| {
                            error!("Failed to store provider in session: {e}");
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;
                    params.oidc_provider
                }
                Err(_e) => match session.get(SESSION_PROVIDER_KEY).await {
                    Ok(Some(provider_id)) => provider_id,
                    _ => {
                        error!("Missing OIDC provider in session");
                        return Err(StatusCode::BAD_REQUEST);
                    }
                },
            };

            let Extension(cache) =
                parts
                    .extract::<Extension<ProvidersCache>>()
                    .await
                    .map_err(|e| {
                        error!("Failed to get providers cache: {e}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
            if let Some(client) = cache.get_provider(&provider_id) {
                return Ok(client);
            }

            let provider_config = state.get_oidc_provider(&provider_id);
            if let Some(provider) = provider_config {
                let redirect_url = state.build_url("auth/callback").map_err(|e| {
                    error!("Failed to build auth callback URL: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
                let client = OIDCClient::discover(&provider, redirect_url)
                    .await
                    .map_err(|e| {
                        error!("Failed to discover OIDC provider {}: {}", provider_id, e);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                cache.set_provider(&provider_id, client.clone());
                Ok(client)
            } else {
                error!("Unknown OIDC provider: {}", provider_id);
                Err(StatusCode::BAD_REQUEST)
            }
        }
    }
}

#[derive(Clone)]
pub struct ProvidersCache {
    providers: Arc<RwLock<HashMap<String, OIDCClient>>>,
}

impl ProvidersCache {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_provider(&self, name: impl AsRef<str>) -> Option<OIDCClient> {
        self.providers.read().unwrap().get(name.as_ref()).cloned()
    }

    pub fn set_provider(&self, name: impl Into<String>, client: OIDCClient) {
        self.providers.write().unwrap().insert(name.into(), client);
    }
}

pub async fn login(client: OIDCClient, session: Session) -> Result<impl IntoResponse, StatusCode> {
    let (url, _secrets) = client.auth_url();
    session
        .insert(SESSION_SECRETS_KEY, _secrets)
        .await
        .map_err(|e| {
            error!("Failed to store secrets in session: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Redirect::temporary(url.as_str()))
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
    pub iss: String,
    pub session_state: Option<String>,
}

pub async fn callback(
    client: OIDCClient,
    session: Session,
    State(state): State<AppState>,
    user_registry: UserRepository,
    Query(params): Query<CallbackQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    debug!("Received callback: {:#?}", params);
    let secrets = session
        .get::<OIDCSecrets>(SESSION_SECRETS_KEY)
        .await
        .map_err(|e| {
            error!("Failed to get secrets from session: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or_else(|| {
            error!("Cannot retrieve session in callback");
            StatusCode::BAD_REQUEST
        })?;

    let token = client
        .token(params.code, params.state, secrets)
        .await
        .map_err(|e| {
            error!("Failed to get token: {e}");
            StatusCode::BAD_REQUEST
        })?;
    debug!("Token: {:#?}", token.claims);
    let user_info = UserClaim::try_from(&token).map_err(|e| {
        error!("Failed to get user info: {e}");
        StatusCode::BAD_REQUEST
    })?;

    match user_registry.find_by_email(&user_info.email).await.ok() {
        Some(known_user) => {
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
        None => {
            //TODO: consider allowing authenticated users with no roles
            warn!("Unknown user: {}", user_info.email);
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
}

pub async fn token(
    session: Session,
    cookies: Cookies,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let user = session.get::<User>(SESSION_USER_KEY).await.map_err(|e| {
        error!("Failed to get user from session: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some(known_user) = user {
        let token = ApiClaim::new_expired(
            known_user.id.to_string(),
            known_user.roles.iter().map(|v| v.into_iter()).flatten(),
        );

        let signed_token = state.tokens().issue(token).map_err(|e| {
            error!("Failed to issue token: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let cookie = Cookie::build((TOKEN_COOKIE_NAME, signed_token.clone()))
            .http_only(true)
            .secure(true)
            .path("/")
            .same_site(SameSite::Lax)
            .expires(Expiration::DateTime(
                OffsetDateTime::now_utc() + state.tokens().default_validity(),
            ));

        cookies.add(cookie.into());

        Ok(signed_token)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

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
        .route("/login", get(login))
        .route("/callback", get(callback))
        .route("/logout", get(logout))
        .route("/token", get(token))
        .layer(session_layer)
        .layer(Extension(ProvidersCache::new()))
}
