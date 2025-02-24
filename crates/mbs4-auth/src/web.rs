use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, RwLock},
};

use axum::{
    extract::{FromRequestParts, Query},
    response::{IntoResponse, Redirect},
    routing::get,
    Extension, RequestPartsExt,
};
use http::StatusCode;
use mbs4_types::app::AppState;
use serde::Deserialize;
use tower_sessions::Session;
use tracing::error;

use crate::oidc::{OIDCClient, OIDCSecrets};

const SESSION_SECRETS_KEY: &str = "oidc_secrets";

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
            let Query(params) = Query::<LoginParams>::from_request_parts(parts, state)
                .await
                .map_err(|_e| StatusCode::BAD_REQUEST)?;
            let Extension(cache) =
                parts
                    .extract::<Extension<ProvidersCache>>()
                    .await
                    .map_err(|e| {
                        error!("Failed to get providers cache: {e}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
            if let Some(client) = cache.get_provider(&params.oidc_provider) {
                return Ok(client);
            }
            let provider_config = state.get_oidc_provider(&params.oidc_provider);
            if let Some(provider) = provider_config {
                let client = OIDCClient::discover(&provider, "http://localhost:3000")
                    .await
                    .map_err(|e| {
                        error!(
                            "Failed to discover OIDC provider {}: {}",
                            params.oidc_provider, e
                        );
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                cache.set_provider(&params.oidc_provider, client.clone());
                Ok(client)
            } else {
                error!("Unknown OIDC provider: {}", params.oidc_provider);
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

pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

pub async fn callback(
    client: OIDCClient,
    session: Session,
    Query(params): Query<CallbackQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let secrets = session
        .get::<OIDCSecrets>(SESSION_SECRETS_KEY)
        .await
        .map_err(|e| {
            error!("Failed to get secrets from session: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::BAD_REQUEST)?;

    Ok(Redirect::temporary("url.as_str()"))
}

pub fn auth_router() -> axum::Router<AppState> {
    let session_store = tower_sessions::MemoryStore::default();
    let session_layer = tower_sessions::SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(tower_sessions::Expiry::OnInactivity(
            time::Duration::seconds(60),
        ));
    axum::Router::new()
        .route("/login", get(login))
        .layer(session_layer)
        .layer(Extension(ProvidersCache::new()))
}
