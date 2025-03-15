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
}
