use crate::{
    auth::token::{create_token, set_token_cookie},
    state::AppState,
};
use axum::{
    extract::{ConnectInfo, FromRequest as _, Query, State},
    response::{IntoResponse, Redirect},
    routing::get,
    Extension, Form, Json,
};
use cookie::{Cookie, Expiration};
use http::{HeaderMap, StatusCode};
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use time::OffsetDateTime;
use tower_cookies::Cookies;
use tower_sessions::Session;
use tracing::{debug, error, warn};

pub mod oidc;
pub mod rate_limit;
pub mod token;

const SESSION_COOKIE_NAME: &str = "mbs4";
const TOKEN_COOKIE_NAME: &str = "mbs4_token";
const SESSION_USER_KEY: &str = "user";
const SESSION_EXPIRY_SECS: i64 = 3600;

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

    let expired_date = OffsetDateTime::now_utc() - time::Duration::days(1);

    let remove_cookie = |cookie_name: &'static str| {
        if let Some(_existing_cookie) = cookies.get(cookie_name) {
            let c = Cookie::build((cookie_name, ""))
                .http_only(true)
                .secure(true)
                .path("/")
                .expires(Expiration::DateTime(expired_date));
            cookies.add(c.into());
        }
    };

    remove_cookie(TOKEN_COOKIE_NAME);
    remove_cookie(SESSION_COOKIE_NAME);

    Ok(Redirect::temporary(redirect_url.as_str()))
}

/// Builds authentication router - must be nested on /auth path!
pub fn auth_router() -> axum::Router<AppState> {
    let session_store = tower_sessions::MemoryStore::default();
    let session_layer = tower_sessions::SessionManagerLayer::new(session_store)
        .with_name(SESSION_COOKIE_NAME)
        .with_secure(true)
        .with_http_only(true)
        .with_same_site(cookie::SameSite::Lax) // Lax is needed for OIDC
        .with_expiry(tower_sessions::Expiry::OnInactivity(
            time::Duration::seconds(SESSION_EXPIRY_SECS),
        ));
    axum::Router::new()
        .route("/login", get(oidc::login).post(db_login))
        .route("/callback", get(oidc::callback))
        .route("/logout", get(logout))
        .route("/token", get(token::token))
        .route("/providers", get(oidc::known_providers))
        .layer(session_layer)
        .layer(Extension(oidc::ProvidersCache::new()))
}

#[cfg(feature = "openapi")]
pub fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    #[derive(utoipa::OpenApi)]
    #[openapi(paths(db_login, oidc::known_providers, oidc::login))]
    struct ApiDocs;

    ApiDocs::openapi()
}

/// Parses the `for=` directive from a `Forwarded` header value (RFC 7239).
/// Handles IPv4 (`for=1.2.3.4`), IPv6 in brackets (`for="[::1]"`),
/// IPv6 with port (`for="[::1]:4711"`), quoted forms, and obfuscated
/// identifiers like `_hidden` (returned as `None`).
fn parse_forwarded_ip(value: &str) -> Option<IpAddr> {
    // A header may carry multiple hops separated by commas; use the first.
    let first_hop = value.split(',').next()?;
    for directive in first_hop.split(';') {
        if let Some((key, val)) = directive.trim().split_once('=') {
            if key.trim().eq_ignore_ascii_case("for") {
                let val = val.trim().trim_matches('"');
                // IPv6: [addr] or [addr]:port — extract just the address part.
                let val = if val.starts_with('[') {
                    val.strip_prefix('[')
                        .and_then(|s| s.split_once(']'))
                        .map(|(addr, _port)| addr)
                        .unwrap_or(val)
                } else {
                    val
                };
                return val.parse().ok();
            }
        }
    }
    None
}

/// Extracts the real client IP from request headers and the TCP connection.
///
/// Priority:
///   1. `Forwarded` (RFC 7239, newer standard)
///   2. `X-Forwarded-For` (de-facto standard)
///   3. Direct TCP connection address
fn client_ip(headers: &HeaderMap, connect_info: &SocketAddr) -> IpAddr {
    headers
        .get("forwarded")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_forwarded_ip)
        .or_else(|| {
            headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.trim().parse().ok())
        })
        .unwrap_or_else(|| connect_info.ip())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

    // ── helpers ──────────────────────────────────────────────────────────────

    fn ipv4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    fn ipv6(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16, g: u16, h: u16) -> IpAddr {
        IpAddr::V6(Ipv6Addr::new(a, b, c, d, e, f, g, h))
    }

    /// 127.0.0.1:9999 — used as the fallback ConnectInfo address in client_ip tests.
    fn loopback() -> SocketAddr {
        "127.0.0.1:9999".parse().unwrap()
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                http::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                http::header::HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    // ── parse_forwarded_ip ───────────────────────────────────────────────────

    // Forwarded: for="_mdn"  — obfuscated identifier, not an IP
    #[test]
    fn forwarded_obfuscated_identifier_returns_none() {
        assert_eq!(parse_forwarded_ip("for=\"_mdn\""), None);
    }

    // Forwarded: For="[2001:db8:cafe::17]:4711"  — case-insensitive key, IPv6 with port
    #[test]
    fn forwarded_ipv6_with_port() {
        assert_eq!(
            parse_forwarded_ip("For=\"[2001:db8:cafe::17]:4711\""),
            Some(ipv6(0x2001, 0x0db8, 0xcafe, 0, 0, 0, 0, 0x17))
        );
    }

    // Forwarded: for=192.0.2.60;proto=http;by=203.0.113.43  — semicolon-separated directives
    #[test]
    fn forwarded_ipv4_with_extra_directives() {
        assert_eq!(
            parse_forwarded_ip("for=192.0.2.60;proto=http;by=203.0.113.43"),
            Some(ipv4(192, 0, 2, 60))
        );
    }

    // Forwarded: for=192.0.2.43, for=198.51.100.17  — multiple hops, use first
    #[test]
    fn forwarded_multi_hop_uses_first() {
        assert_eq!(
            parse_forwarded_ip("for=192.0.2.43, for=198.51.100.17"),
            Some(ipv4(192, 0, 2, 43))
        );
    }

    // Forwarded: for="[2001:db8:cafe::17]"  — IPv6 in brackets, no port
    #[test]
    fn forwarded_ipv6_brackets_no_port() {
        assert_eq!(
            parse_forwarded_ip("for=\"[2001:db8:cafe::17]\""),
            Some(ipv6(0x2001, 0x0db8, 0xcafe, 0, 0, 0, 0, 0x17))
        );
    }

    // ── client_ip: X-Forwarded-For only ──────────────────────────────────────

    // X-Forwarded-For: 2001:db8:85a3:8d3:1319:8a2e:370:7348
    #[test]
    fn xff_single_ipv6() {
        let h = headers(&[("x-forwarded-for", "2001:db8:85a3:8d3:1319:8a2e:370:7348")]);
        assert_eq!(
            client_ip(&h, &loopback()),
            ipv6(0x2001, 0x0db8, 0x85a3, 0x08d3, 0x1319, 0x8a2e, 0x0370, 0x7348)
        );
    }

    // X-Forwarded-For: 203.0.113.195
    #[test]
    fn xff_single_ipv4() {
        let h = headers(&[("x-forwarded-for", "203.0.113.195")]);
        assert_eq!(client_ip(&h, &loopback()), ipv4(203, 0, 113, 195));
    }

    // X-Forwarded-For: 203.0.113.195, 2001:db8:85a3:8d3:1319:8a2e:370:7348
    #[test]
    fn xff_multi_hop_uses_first() {
        let h = headers(&[(
            "x-forwarded-for",
            "203.0.113.195, 2001:db8:85a3:8d3:1319:8a2e:370:7348",
        )]);
        assert_eq!(client_ip(&h, &loopback()), ipv4(203, 0, 113, 195));
    }

    // ── client_ip: Forwarded takes priority over X-Forwarded-For ────────────

    // X-Forwarded-For: 192.0.2.172
    // Forwarded: for=192.0.2.172
    #[test]
    fn forwarded_takes_priority_over_xff_same_ip() {
        let h = headers(&[
            ("x-forwarded-for", "10.0.0.1"),
            ("forwarded", "for=192.0.2.172"),
        ]);
        assert_eq!(client_ip(&h, &loopback()), ipv4(192, 0, 2, 172));
    }

    // X-Forwarded-For: 192.0.2.43, 2001:db8:cafe::17
    // Forwarded: for=192.0.2.43, for="[2001:db8:cafe::17]"
    #[test]
    fn forwarded_takes_priority_over_xff_multi_hop() {
        let h = headers(&[
            ("x-forwarded-for", "10.0.0.99, 10.0.0.100"),
            ("forwarded", "for=192.0.2.43, for=\"[2001:db8:cafe::17]\""),
        ]);
        assert_eq!(client_ip(&h, &loopback()), ipv4(192, 0, 2, 43));
    }

    // ── client_ip: fallback to ConnectInfo ───────────────────────────────────

    #[test]
    fn no_headers_falls_back_to_connect_info() {
        let h = HeaderMap::new();
        assert_eq!(client_ip(&h, &loopback()), ipv4(127, 0, 0, 1));
    }

    // Forwarded: for="_mdn" — obfuscated, falls through to X-Forwarded-For
    #[test]
    fn forwarded_obfuscated_falls_back_to_xff() {
        let h = headers(&[
            ("forwarded", "for=\"_mdn\""),
            ("x-forwarded-for", "203.0.113.195"),
        ]);
        assert_eq!(client_ip(&h, &loopback()), ipv4(203, 0, 113, 195));
    }
}

#[derive(serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
struct LoginCredentials {
    email: String,
    password: String,
}

pub async fn after_ok_login<S>(
    state: &AppState,
    session: &Session,
    known_user: mbs4_dal::user::User,
    redirect_path: S,
) -> Result<axum::response::Redirect, StatusCode>
where
    S: AsRef<str>,
{
    session
        .insert(SESSION_USER_KEY, known_user)
        .await
        .map_err(|e| {
            error!("Failed to store user in session: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let redirect_path = redirect_path.as_ref();

    // Only allow paths (must start with '/' and must not contain '://' to prevent open redirect)
    let safe_redirect_path = if redirect_path.starts_with('/') && !redirect_path.contains("://") {
        redirect_path
    } else {
        warn!("Rejecting unsafe redirect path: {redirect_path}");
        "/"
    };

    let mut redirect_url = state.build_url(safe_redirect_path).map_err(|e| {
        error!("Failed to build redirect URL: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let tr_token = state.tokens().create_tr_token().map_err(|e| {
        error!("Failed to create TR token: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    redirect_url.query_pairs_mut().append_pair("trt", &tr_token);

    Ok(Redirect::to(redirect_url.as_str()))
}

#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature="openapi", into_params(parameter_in = Query))]
pub struct DbLoginParams {
    redirect: Option<String>,
    token: Option<bool>,
}

pub enum LoginResponse {
    Redirect(axum::response::Redirect),
    Token(String),
}

impl IntoResponse for LoginResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            LoginResponse::Redirect(r) => r.into_response(),
            LoginResponse::Token(t) => t.into_response(),
        }
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(post, path = "/login", tag = "auth", 
operation_id = "loginLocally",
params(DbLoginParams),
request_body(description = "User credentials", content(
(LoginCredentials = "application/x-www-form-urlencoded"),
(LoginCredentials = "application/json" ),
)),
responses((status = StatusCode::OK, description = "Success", content_type = "text/plain"),
(status = StatusCode::SEE_OTHER, description = "Success andRedirect"))))]
pub async fn db_login(
    state: State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    user_registry: mbs4_dal::user::UserRepository,
    session: Session,
    cookies: Cookies,
    Query(DbLoginParams { redirect, token }): Query<DbLoginParams>,
    request: axum::extract::Request,
) -> Result<impl IntoResponse, StatusCode> {
    let ip = client_ip(request.headers(), &addr);
    if !state.login_limiter().check_and_record(ip).await {
        warn!("Rate limit exceeded for login from {ip}");
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

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
    let return_token = token.unwrap_or_default();
    let token = create_token(&state, user.clone()).map_err(|e| {
        error!("Failed to issue token: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    set_token_cookie(token.clone(), &cookies, &state);
    if return_token {
        Ok(LoginResponse::Token(token))
    } else {
        let redirect =
            after_ok_login(&state, &session, user, redirect.unwrap_or("/".to_string())).await?;
        Ok(LoginResponse::Redirect(redirect))
    }
}
