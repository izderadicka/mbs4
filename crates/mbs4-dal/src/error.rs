pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Database error: {0}")]
    DatabaseError(sqlx::Error),

    #[error("User password error: {0}")]
    UserPasswordError(#[from] argon2::password_hash::Error),

    #[error("Record not found: {0}")]
    RecordNotFound(String),

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Missing version")]
    MissingVersion,

    #[error("Failed update")]
    FailedUpdate { id: i64, version: i64 },

    #[error("Unique violation, conflicting record already exists")]
    UniqueViolation,

    #[error("Invalid order by field: {0}")]
    InvalidOrderByField(String),

    #[error("Async error: {0}")]
    AsyncError(#[from] tokio::task::JoinError),

    #[error("DB reference error: {0}")]
    DBReferenceError(sqlx::Error),

    #[error("Invalid entity: {0}")]
    InvalidEntity(String),

    #[error("Invalid filter: {0}")]
    InvalidFilter(String),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::RowNotFound => Error::RecordNotFound("".to_string()),
            sqlx::Error::Database(db_error) if db_error.is_unique_violation() => {
                Error::UniqueViolation
            }

            _ => Error::DatabaseError(error),
        }
    }
}
