use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow, Repository)]
pub struct Series {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    pub title: String,
    #[garde(length(min = 1, max = 5000))]
    #[omit(short, sort)]
    pub description: Option<String>,
    #[garde(range(min = 0))]
    #[spec(version)]
    pub version: i64,
    #[spec(created_by)]
    pub created_by: Option<String>,
    #[spec(created)]
    pub created: time::PrimitiveDateTime,
    #[spec(modified)]
    pub modified: time::PrimitiveDateTime,
}
