pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("User password error: {0}")]
    UserPasswordError(#[from] argon2::password_hash::Error),

    #[error("Record not found: {0}")]
    RecordNotFound(String),

    #[error("Invalid credentials")]
    InvalidCredentials,
}
