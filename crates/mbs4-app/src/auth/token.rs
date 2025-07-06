use std::{
    sync::Arc,
    task::{Context, Poll},
};

use crate::state::AppState;
use axum::{
    extract::{FromRequestParts, Request, State},
    response::{IntoResponse, Response},
    Extension, RequestPartsExt,
};
use axum_extra::TypedHeader;
use cookie::{Cookie, Expiration, SameSite};
use futures::future::BoxFuture;
use headers::{authorization::Bearer, Authorization, HeaderMapExt};
use http::{request::Parts, StatusCode};
use mbs4_dal::user::User;
use mbs4_types::claim::{ApiClaim, Authorization as _, Role};
use time::OffsetDateTime;
use tower::{Layer, Service};
use tower_cookies::Cookies;
use tower_sessions::Session;
use tracing::{debug, error, field::debug};

use super::{SESSION_USER_KEY, TOKEN_COOKIE_NAME};

#[derive(Clone)]
pub struct RequiredRolesLayer {
    roles: Arc<Vec<Role>>,
}

impl RequiredRolesLayer {
    pub fn new(roles: impl IntoIterator<Item = impl Into<Role>>) -> Self {
        Self {
            roles: Arc::new(roles.into_iter().map(Into::into).collect()),
        }
    }
}

impl<S> Layer<S> for RequiredRolesLayer {
    type Service = RequiredRoles<S>;

    fn layer(&self, service: S) -> Self::Service {
        RequiredRoles {
            inner: service,
            roles: self.roles.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RequiredRoles<S> {
    inner: S,
    roles: Arc<Vec<Role>>,
}

impl<S, B> Service<Request<B>> for RequiredRoles<S>
where
    S: Service<Request<B>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let roles = self.roles.clone();
        // Make clone of inner service, which is ready
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(async move {
            if let Some(claim) = req.extensions().get::<ApiClaim>() {
                if !claim.has_any_role_ref(&*roles) {
                    debug!("User token does not have required roles");
                    return Ok(StatusCode::FORBIDDEN.into_response());
                }
                inner.call(req).await
            } else {
                error!("User claim not found in request, probably Token Layer not applied");
                Ok(StatusCode::UNAUTHORIZED.into_response())
            }
        })
    }
}

#[derive(Clone)]
pub struct TokenLayer {
    state: AppState,
}

impl TokenLayer {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for TokenLayer {
    type Service = TokenExtractor<S>;

    fn layer(&self, service: S) -> Self::Service {
        TokenExtractor {
            inner: service,
            state: self.state.clone(),
        }
    }
}

#[derive(Clone)]
pub struct TokenExtractor<S> {
    inner: S,
    state: AppState,
}

impl<S, B> Service<Request<B>> for TokenExtractor<S>
where
    S: Service<Request<B>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: Request<B>) -> Self::Future {
        // Make clone of inner service, which is ready
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let state = self.state.clone();

        Box::pin(async move {
            // Extract the token from the Authorization header
            if request.extensions().get::<ApiClaim>().is_some() {
                debug("Token already extracted");
                return inner.call(request).await;
            }
            let mut token = request
                .headers()
                .typed_get::<Authorization<Bearer>>()
                .map(|header| header.0.token().to_string());

            if token.is_none() {
                debug!("No token found in headers");
                let Some(cookies) = request.extensions().get::<Cookies>().cloned() else {
                    tracing::error!(
                        "missing cookies request extension, is cookie middleware enabled?"
                    );
                    return Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response());
                };
                token = cookies
                    .get(TOKEN_COOKIE_NAME)
                    .map(|t| t.value().to_string());
            }

            match token {
                Some(token) => {
                    debug("Token found, validating");
                    match state.tokens().validate::<ApiClaim>(&token) {
                        Ok(claim) => {
                            request.extensions_mut().insert(claim);
                        }
                        Err(e) => {
                            error!("Failed to validate token: {}", e);
                            return Ok(StatusCode::UNAUTHORIZED.into_response());
                        }
                    }
                    // store as extension for later use
                }
                None => {
                    debug!("No token found");
                    return Ok(StatusCode::UNAUTHORIZED.into_response());
                }
            }

            // Continue with the request
            inner.call(request).await
        })
    }
}

impl FromRequestParts<AppState> for ApiClaim {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // check if we already have a token in extensions
        if let Ok(existing_claim) = parts.extract::<Extension<ApiClaim>>().await {
            return Ok(existing_claim.0);
        }
        let mut header_token = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .ok()
            .map(|h| h.0.token().to_string());

        if header_token.is_none() {
            debug!("No token found in headers");
            let cookies = parts.extract::<Cookies>().await.map_err(|e| {
                error!("Cannot get cookies: {}", e.1);
                e.0
            })?;
            header_token = cookies.get(TOKEN_COOKIE_NAME).map(|t| t.to_string());
        }

        match header_token {
            Some(token) => {
                debug("Token found, validating");
                let claim = state.tokens().validate::<ApiClaim>(&token).map_err(|e| {
                    error!("Failed to validate token: {}", e);
                    StatusCode::UNAUTHORIZED
                })?;
                // store as extension for later use
                parts.extensions.insert(claim.clone());
                Ok(claim)
            }
            None => {
                debug!("No token found");
                Err(StatusCode::UNAUTHORIZED)
            }
        }
    }
}

pub(crate) fn create_token(state: &AppState, known_user: User) -> anyhow::Result<String> {
    let token = ApiClaim::new_expired(
        known_user.email,
        known_user.roles.iter().flat_map(|v| {
            v.iter().filter_map(|role_name| {
                role_name
                    .parse::<Role>()
                    .map_err(|e| error!("Failed to parse role name: {e}"))
                    .ok()
            })
        }),
    );

    let signed_token = state.tokens().issue(token)?;
    Ok(signed_token)
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
        let signed_token = create_token(&state, known_user).map_err(|e| {
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
