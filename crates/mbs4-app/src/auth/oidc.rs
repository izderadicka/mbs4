use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{auth::after_ok_login, state::AppState};
use anyhow::{anyhow, bail};
use axum::{
    extract::{FromRequestParts, Query, State},
    response::{IntoResponse, Redirect},
    Extension, Json, RequestPartsExt,
};
use http::{request::Parts, StatusCode};
use mbs4_dal::user::UserRepository;
use mbs4_types::claim::UserClaim;
use serde::Deserialize;
use tower_sessions::Session;
use tracing::{debug, error, warn};

use mbs4_auth::oidc::{IDToken, OIDCClient, OIDCSecrets};

const SESSION_SECRETS_KEY: &str = "oidc_secrets";
const SESSION_PROVIDER_KEY: &str = "oidc_provider";

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi",into_params(parameter_in = Query))]
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
            Err(_e) => provider_id(&session).await.ok_or_else(|| {
                error!("Missing OIDC provider in session");
                StatusCode::BAD_REQUEST
            })?,
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

        match cache.prepare_provider(&provider_id, state).await {
            Ok(Some(client)) => Ok(client),
            Ok(None) => {
                error!("Unknown OIDC provider: {}", provider_id);
                Err(StatusCode::BAD_REQUEST)
            }
            Err(e) => {
                error!("Failed to prepare OIDC provider: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
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

    pub fn remove_provider(&self, name: impl AsRef<str>) {
        self.providers.write().unwrap().remove(name.as_ref());
    }

    pub async fn prepare_provider(
        &self,
        provider_id: &str,
        state: &AppState,
    ) -> anyhow::Result<Option<OIDCClient>> {
        let provider_config = state.get_oidc_provider(&provider_id);
        if let Some(provider) = provider_config {
            let redirect_url = state.build_backend_url("auth/callback")?;
            let client = OIDCClient::discover(&provider, redirect_url).await?;
            self.set_provider(provider_id, client.clone());
            Ok(Some(client))
        } else {
            Ok(None)
        }
    }
}

#[cfg_attr(
    feature = "openapi",
    utoipa::path(
        get,
        path = "/login",
        tag = "auth",
        operation_id = "startOIDCLogin",
        params(LoginParams),
    )
)]
pub async fn login(client: OIDCClient, session: Session) -> Result<impl IntoResponse, StatusCode> {
    let (url, secrets) = client.auth_url_with_scopes(["email", "profile"]);
    session
        .insert(SESSION_SECRETS_KEY, secrets)
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
    pub iss: Option<String>,
    pub session_state: Option<String>,
}

async fn provider_id(session: &Session) -> Option<String> {
    session
        .get::<String>(SESSION_PROVIDER_KEY)
        .await
        .inspect_err(|e| error!("Error getting provider from session: {e}"))
        .ok()
        .flatten()
}

async fn refresh_provider(
    cache: &ProvidersCache,
    state: &AppState,
    session: &Session,
) -> anyhow::Result<(String, OIDCClient)> {
    let oidc_provider = provider_id(session)
        .await
        .ok_or_else(|| anyhow!("Failed to get provider id from session"))?;
    let client = cache
        .prepare_provider(&oidc_provider, state)
        .await?
        .ok_or_else(|| anyhow!("Unknown OIDC provider: {}", oidc_provider))?;
    Ok((oidc_provider, client))
}

async fn get_secrets(session: &Session) -> Result<OIDCSecrets, StatusCode> {
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
    Ok(secrets)
}

async fn get_token_with_retry(
    client: OIDCClient,
    providers_cache: ProvidersCache,
    session: &Session,
    state: &AppState,
    params: CallbackQuery,
    secrets: OIDCSecrets,
) -> anyhow::Result<IDToken> {
    let nonce = secrets.nonce();
    match client
        .retrieve_id_token(params.code.clone(), params.state.clone(), secrets.clone())
        .await
    {
        Ok(token_response) => {
            debug!("Token response: {token_response:#?}");
            match client.verify_id_token(token_response.clone(), &nonce).await {
                Ok(token) => Ok(token),
                Err(e) => {
                    error!("Failed to validate token: {e}");
                    if let mbs4_auth::Error::MissingKeyError = e {
                        debug!("No matching key for token validation, try refresh client");
                        let (oidc_provider, new_client) =
                            refresh_provider(&providers_cache, state, session).await?;
                        match new_client.verify_id_token(token_response, &nonce).await {
                            Ok(token) => Ok(token),
                            Err(e) => {
                                providers_cache.remove_provider(&oidc_provider);
                                debug!(
                                "Removed provider {} from cache, because of token validation error",
                                oidc_provider
                            );
                                bail!("Failed to verify token in retry: {e}");
                            }
                        }
                    } else {
                        bail!("Token validation error: {e}")
                    }
                }
            }
        }
        Err(e) => {
            bail!("Failed to retrieve token: {e}");
        }
    }
}

pub async fn callback(
    client: OIDCClient,
    Extension(providers_cache): Extension<ProvidersCache>,
    session: Session,
    State(state): State<AppState>,
    user_registry: UserRepository,
    Query(params): Query<CallbackQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    debug!("Received callback: {:#?}", params);
    let secrets = get_secrets(&session).await?;

    let token = get_token_with_retry(client, providers_cache, &session, &state, params, secrets)
        .await
        .inspect_err(|e| error!("Error retrieving ID token {e}"))
        .map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?;
    debug!("Token: {:#?}", token.claims);
    let user_info = UserClaim::try_from(&token).map_err(|e| {
        error!("Failed to get user info from token: {e}");
        StatusCode::UNAUTHORIZED
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

#[cfg_attr(feature = "openapi", utoipa::path(get, path = "/providers", tag = "auth", 
operation_id = "getProviders",
responses((status = StatusCode::OK, description = "Success", body = Vec<String> ),)))]
pub async fn known_providers(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let known_providers = state.known_oidc_providers();
    Ok(Json(known_providers))
}
