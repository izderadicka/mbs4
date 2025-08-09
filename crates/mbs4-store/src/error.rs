pub type StoreResult<T> = std::result::Result<T, StoreError>;

#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("Invalid path")]
    InvalidPath,
    #[error("Cannot create on provided path")]
    PathConflict,
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Task join error: {0}")]
    TaskJoinError(#[from] tokio::task::JoinError),
    #[error("Not found: {0:?}")]
    NotFound(String),
}
