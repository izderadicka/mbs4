use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{auth::after_ok_login, state::AppState};
use axum::{
    extract::{FromRequestParts, Query, State},
    response::{IntoResponse, Redirect},
    Extension, RequestPartsExt,
};
use http::{request::Parts, StatusCode};
use mbs4_dal::user::UserRepository;
use mbs4_types::claim::UserClaim;
use serde::Deserialize;
use tower_sessions::Session;
use tracing::{debug, error, warn};

use mbs4_auth::oidc::{OIDCClient, OIDCSecrets};

const SESSION_SECRETS_KEY: &str = "oidc_secrets";
const SESSION_PROVIDER_KEY: &str = "oidc_provider";

#[derive(Debug, Deserialize)]
pub struct LoginParams {
    oidc_provider: String,
}

impl FromRequestParts<AppState> for OIDCClient {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
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

        let Extension(cache) = parts
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

#[derive(Clone)]
pub struct ProvidersCache {
    providers: Arc<RwLock<HashMap<String, OIDCClient>>>,
}

impl Default for ProvidersCache {
    fn default() -> Self {
        Self::new()
    }
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

    match user_registry.find_by_email(&user_info.email).await {
        Ok(known_user) => after_ok_login(&state, &session, known_user, "/").await,
        Err(_) => {
            //TODO: consider allowing authenticated users with no roles
            warn!("Unknown user: {}", user_info.email);
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}
