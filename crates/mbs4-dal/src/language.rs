use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Repository, sqlx::FromRow, utoipa::ToSchema)]
pub struct Language {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    pub name: String,
    #[garde(length(min = 2, max = 4))]
    pub code: String,
    #[garde(range(min = 0))]
    #[spec(version)]
    pub version: i64,
}
