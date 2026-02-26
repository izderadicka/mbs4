use jsonwebtoken::errors::Error as JwtError;

pub type Result<T, E = Error> = std::result::Result<T, E>;

type GenericError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("JWT error: {0}")]
    JwtError(#[from] JwtError),
    #[error("Token retrieve token error: {0}: {1:?}")]
    TrTokenError(&'static str, #[source] Option<GenericError>),
    #[error("OIDC claim validation error: {0}")]
    OidcClaimError(#[from] openidconnect::ClaimsVerificationError),
    #[error("OIDC error: {0}: {1:?}")]
    OidcError(&'static str, #[source] Option<GenericError>),
    #[error("ID token error: {0}")]
    IdTokenError(&'static str),
    #[error("Configuration error: {0}: {1:?}")]
    ConfigError(&'static str, #[source] Option<GenericError>),
}

impl Error {
    pub(crate) fn tr_token_error<E>(msg: &'static str, e: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::TrTokenError(msg, Some(Box::new(e)))
    }

    pub(crate) fn tr_token_error_msg(msg: &'static str) -> Self {
        Error::TrTokenError(msg, None)
    }

    pub(crate) fn oidc_error<E>(msg: &'static str, e: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::OidcError(msg, Some(Box::new(e)))
    }

    pub(crate) fn oidc_error_msg(msg: &'static str) -> Self {
        Error::OidcError(msg, None)
    }

    pub fn config_error<E>(msg: &'static str, e: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::ConfigError(msg, Some(Box::new(e)))
    }
}

// these should be included in TRTokenError

// #[error("Time value error: {0}")]
//     TimeValueError(#[from] SystemTimeError),
//     #[error("Invalid HMAC length: {0}")]
//     InvalidHmacLength(#[from] InvalidLength),
//     #[error("Invalid TR token: {0}")]
//     InvalidTrToken(String),
//     #[error("Base64 error: {0}")]
//     Base64Error(#[from] base64::DecodeError),
