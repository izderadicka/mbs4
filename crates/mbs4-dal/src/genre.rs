use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow, Repository)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Genre {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    name: String,
    #[garde(range(min = 0))]
    #[spec(version)]
    pub version: i64,
}
