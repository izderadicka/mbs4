use crate::{dal::user::User, state::AppState};
use axum::{
    extract::{FromRequestParts, State},
    response::IntoResponse,
    RequestPartsExt,
};
use axum_extra::TypedHeader;
use cookie::{Cookie, Expiration, SameSite};
use headers::{authorization::Bearer, Authorization};
use http::{request::Parts, StatusCode};
use mbs4_types::claim::ApiClaim;
use time::OffsetDateTime;
use tower_cookies::Cookies;
use tower_sessions::Session;
use tracing::{debug, error};

use super::{SESSION_USER_KEY, TOKEN_COOKIE_NAME};

impl FromRequestParts<AppState> for ApiClaim {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let mut header_token = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .ok()
            .map(|h| h.0.token().to_string());

        if header_token.is_none() {
            let cookies = parts.extract::<Cookies>().await.map_err(|e| {
                error!("Cannot get cookies: {}", e.1);
                e.0
            })?;
            header_token = cookies.get(TOKEN_COOKIE_NAME).map(|t| t.to_string());
        }

        match header_token {
            Some(token) => {
                let claim = state.tokens().validate::<ApiClaim>(&token).map_err(|e| {
                    error!("Failed to validate token: {}", e);
                    StatusCode::UNAUTHORIZED
                })?;
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
