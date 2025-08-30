use std::borrow::Cow;

use axum::{
    extract::{multipart::MultipartError, rejection::PathRejection},
    response::IntoResponse,
    Json,
};
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
    #[error("Multipart form error: {0}")]
    MultipartError(#[from] MultipartError),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Unprocessable request: {0}")]
    UnprocessableRequest(String),
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error("Store error: {0}")]
    StoreError(#[from] mbs4_store::error::StoreError),
    #[error("Invalid path: {0}")]
    InvalidPath(#[from] PathRejection),
    #[error("Invalid query: {0}")]
    InvalidQuery(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message): (StatusCode, Cow<str>) = match self {
            ApiError::DatabaseError(error) => match error {
                mbs4_dal::Error::DatabaseError(error) => {
                    tracing::error!("sqlx error: {error}");
                    (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".into())
                }
                mbs4_dal::Error::UniqueViolation => {
                    (StatusCode::CONFLICT, "Resource already exists".into())
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
                mbs4_dal::Error::MissingVersion => {
                    tracing::debug!("Missing version");
                    (StatusCode::BAD_REQUEST, "Missing version".into())
                }
                mbs4_dal::Error::FailedUpdate { id, version } => {
                    tracing::debug!("Failed update: {id} {version}");
                    (StatusCode::CONFLICT, "Failed update".into())
                }
                mbs4_dal::Error::InvalidOrderByField(field) => {
                    tracing::debug!("Invalid order by field: {field}");
                    (StatusCode::BAD_REQUEST, "Invalid order by field".into())
                }
                mbs4_dal::Error::AsyncError(error) => {
                    tracing::error!("Async error: {error}");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Application error".into(),
                    )
                }
                mbs4_dal::Error::DBReferenceError(error) => {
                    tracing::error!("DB reference error: {error}");
                    (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "Invalid references in entity".into(),
                    )
                }
                mbs4_dal::Error::InvalidEntity(msg) => {
                    tracing::error!("Invalid entity: {msg}");
                    (StatusCode::UNPROCESSABLE_ENTITY, "Invalid entity".into())
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

            ApiError::MultipartError(error) => {
                tracing::debug!("Multipart form error: {error}");
                (StatusCode::BAD_REQUEST, "Multipart form error".into())
            }
            ApiError::InvalidRequest(msg) => {
                tracing::debug!("Invalid request: {msg}");
                (StatusCode::BAD_REQUEST, msg.into())
            }
            ApiError::UnprocessableRequest(msg) => {
                tracing::debug!("Unprocessable request: {msg}");
                (StatusCode::UNPROCESSABLE_ENTITY, msg.into())
            }
            ApiError::InternalError(msg) => {
                tracing::error!("Internal error: {msg}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".into())
            }
            ApiError::StoreError(error) => {
                use mbs4_store::error::StoreError;
                match error {
                    StoreError::InvalidPath => {
                        tracing::debug!("Invalid path: {error}");
                        (StatusCode::BAD_REQUEST, "Invalid path".into())
                    }
                    StoreError::NotFound(path) => {
                        tracing::debug!("File not found: {path}");
                        (StatusCode::NOT_FOUND, "File not found".into())
                    }
                    _ => {
                        tracing::error!("Store error: {error}");
                        (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".into())
                    }
                }
            }
            ApiError::InvalidPath(error) => {
                tracing::debug!("Invalid path: {error}");
                (StatusCode::BAD_REQUEST, "Invalid path".into())
            }
            ApiError::InvalidQuery(msg) => {
                tracing::debug!("Invalid query: {msg}");
                (StatusCode::BAD_REQUEST, msg.into())
            }
        };
        let body = serde_json::json!({"error": error_message});
        (status, Json(body)).into_response()
    }
}
