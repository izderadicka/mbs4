use axum::extract::rejection::PathRejection;

pub type StoreResult<T> = std::result::Result<T, StoreError>;

#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("Invalid path")]
    InvalidPath,
    #[error("Cannot create on provoded path")]
    PathConflict,
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Task join error: {0}")]
    TaskJoinError(#[from] tokio::task::JoinError),
    #[error("Multipart error: {0}")]
    MultipartError(#[from] axum::extract::multipart::MultipartError),
    #[error("Axum error: {0}")]
    AxumError(#[from] axum::Error),
    #[error("Not found: {0:?}")]
    NotFound(String),
    #[error("Rejected path: {0:?}")]
    RejectedPath(#[from] PathRejection),
}
