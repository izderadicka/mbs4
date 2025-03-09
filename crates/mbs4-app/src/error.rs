use std::borrow::Cow;

use axum::{response::IntoResponse, Json};
use http::StatusCode;

pub type Error = anyhow::Error; //Box<dyn std::error::Error + Send + Sync + 'static>;
pub type Result<T, E = Error> = std::result::Result<T, E>;

pub type ApiResult<T, E = ApiError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] mbs4_dal::Error),

    #[error("Resource not found: {0}")]
    ResourceNotFound(String),
    #[error("Resource already exists: {0}")]
    ResourceAlreadyExists(String),
    #[error("Application error: {0}")]
    ApplicationError(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message): (StatusCode, Cow<str>) = match self {
            ApiError::DatabaseError(error) => match error {
                mbs4_dal::Error::DatabaseError(error) => {
                    if let mbs4_dal::SqlxError::Database(db_error) = error {
                        if db_error.is_unique_violation() {
                            (StatusCode::CONFLICT, "Resource already exists".into())
                        } else {
                            tracing::error!("Database error: {db_error}");
                            (StatusCode::INTERNAL_SERVER_ERROR, "Database error".into())
                        }
                    } else {
                        tracing::error!("sqlx error: {error}");
                        (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".into())
                    }
                }
                mbs4_dal::Error::UserPasswordError(error) => {
                    tracing::error!("User password error: {error}");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Application error".into(),
                    )
                }
                mbs4_dal::Error::RecordNotFound(_) => {
                    tracing::debug!("Record not found: {error}");
                    (StatusCode::NOT_FOUND, "Resource not found".into())
                }
                mbs4_dal::Error::InvalidCredentials => {
                    (StatusCode::UNAUTHORIZED, "Invalid credentials".into())
                }
            },
            ApiError::ResourceNotFound(r) => (
                StatusCode::NOT_FOUND,
                format!("Resource {r} not found").into(),
            ),
            ApiError::ResourceAlreadyExists(r) => (
                StatusCode::CONFLICT,
                format!("Resource {r} already exists").into(),
            ),
            ApiError::ApplicationError(error) => {
                tracing::error!("Application error: {error}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Application error".into(),
                )
            }
        };
        let body = serde_json::json!({"error": error_message});
        (status, Json(body)).into_response()
    }
}
