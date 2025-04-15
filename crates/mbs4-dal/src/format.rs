use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow, Repository)]
pub struct Format {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    name: String,
    #[garde(length(min = 3, max = 255))]
    mime_type: String,
    #[garde(length(min = 1, max = 32))]
    extension: String,
    #[garde(range(min = 0))]
    #[spec(version)]
    pub version: i64,
}
