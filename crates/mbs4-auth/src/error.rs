use jsonwebtoken::errors::Error as JwtError;

pub type Result<T, E = Error> = std::result::Result<T, E>;

type GenericError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("JWT error: {0}")]
    JwtError(#[from] JwtError),
    #[error("Token retrieve token error: {0}")]
    TRTokenError(&'static str, #[source] Option<GenericError>),
    #[error("OIDC claim validationerror: {0}")]
    OidcClaimError(#[from] openidconnect::ClaimsVerificationError),
}

impl Error {
    pub(crate) fn tr_token_error<E>(msg: &'static str, e: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::TRTokenError(msg, Some(Box::new(e)))
    }

    pub(crate) fn tr_token_error_msg(msg: &'static str) -> Self {
        Error::TRTokenError(msg, None)
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
