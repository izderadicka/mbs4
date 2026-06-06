use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, sqlx::Type)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[sqlx(rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum ConversionBatchEntity {
    Bookshelf,
    Series,
    Author,
}

impl ConversionBatchEntity {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConversionBatchEntity::Bookshelf => "BOOKSHELF",
            ConversionBatchEntity::Series => "SERIES",
            ConversionBatchEntity::Author => "AUTHOR",
        }
    }
}

impl std::fmt::Display for ConversionBatchEntity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow, Repository)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversionBatch {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    pub name: String,
    pub for_entity: Option<ConversionBatchEntity>,
    pub entity_id: Option<i64>,
    pub format_id: i64,
    #[garde(length(min = 1, max = 1023))]
    pub zip_location: Option<String>,

    #[spec(created_by)]
    pub created_by: Option<String>,
    #[spec(created)]
    pub created: time::PrimitiveDateTime,
}
