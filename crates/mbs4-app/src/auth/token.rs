use std::{future::Future, pin::Pin};

use crate::{dal::user::User, state::AppState};
use axum::{
    extract::{FromRequestParts, Request, State},
    middleware::{FromFnLayer, Next},
    response::{IntoResponse, Response},
    Extension, RequestPartsExt,
};
use axum_extra::TypedHeader;
use cookie::{Cookie, Expiration, SameSite};
use headers::{authorization::Bearer, Authorization};
use http::{request::Parts, StatusCode};
use mbs4_types::claim::{ApiClaim, Authorization as _, Role};
use time::OffsetDateTime;
use tower_cookies::Cookies;
use tower_sessions::Session;
use tracing::{debug, error, field::debug};

use super::{SESSION_USER_KEY, TOKEN_COOKIE_NAME};

// pub fn required_roles<T>(
//     roles: Vec<String>,
// ) -> FromFnLayer<impl AsyncFn(ApiClaim, Request, Next) -> Response, (), T>
// where
// {
//     let inner_fn = async move |claim: ApiClaim, req: Request, next: Next| {
//         if !claim.has_any_role(&roles) {
//             return StatusCode::FORBIDDEN.into_response();
//         }
//         next.run(req).await
//     };
//     let midleware = axum::middleware::from_fn(inner_fn);
//     midleware
// }

pub fn required_role<T, S>(
    role: impl Into<Role>,
    state: S,
) -> FromFnLayer<
    impl Fn(ApiClaim, Request, Next) -> Pin<Box<dyn Future<Output = Response>>> + Clone + Send + 'static,
    S,
    T,
> {
    let role: Role = role.into();
    let inner_fn = move |claim: ApiClaim,
                         req: Request,
                         next: Next|
          -> Pin<Box<dyn Future<Output = Response>>> {
        let role: Role = role.clone();
        Box::pin(async move {
            if !claim.has_role(&role) {
                return StatusCode::FORBIDDEN.into_response();
            }
            next.run(req).await
        })
    };
    let midleware = axum::middleware::from_fn_with_state(state, inner_fn);
    midleware
}

pub async fn check_admin(claim: ApiClaim, req: Request, next: Next) -> Response {
    if !claim.has_role("admin") {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(req).await
}

impl FromRequestParts<AppState> for ApiClaim {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // check if we already have a token in extensions
        if let Some(existing_claim) = parts.extract::<Extension<ApiClaim>>().await.ok() {
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
            known_user.roles.iter().flat_map(|v| v.iter()),
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
