pub type Error = anyhow::Error;
pub type Result<T, E = Error> = std::result::Result<T, E>;
pub use garde::Report as ValidationError;
