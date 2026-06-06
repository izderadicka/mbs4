use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

use crate::{Batch, ListingParams, error::Result};

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

impl<'c, E> ConversionBatchRepositoryImpl<E>
where
    for<'a> &'a E: sqlx::Executor<'c, Database = crate::ChosenDB>
        + sqlx::Acquire<'c, Database = crate::ChosenDB>,
{
    /// Paged list of conversion batches. When `created_by` is `Some`, filters
    /// to that user; when `None`, returns batches from all users (admin use).
    pub async fn list_for_user(
        &self,
        created_by: Option<&str>,
        params: ListingParams,
    ) -> Result<Batch<ConversionBatch>> {
        let order = params.ordering(VALID_ORDER_FIELDS)?;
        let order = if order.is_empty() {
            "ORDER BY created DESC".to_string()
        } else {
            order
        };
        let rows = match created_by {
            Some(user) => {
                let sql = format!(
                    "SELECT * FROM conversion_batch WHERE created_by = ? {order} LIMIT ? OFFSET ?"
                );
                sqlx::query_as::<_, ConversionBatch>(&sql)
                    .bind(user)
                    .bind(params.limit)
                    .bind(params.offset)
                    .fetch_all(&self.executor)
                    .await?
            }
            None => {
                let sql = format!("SELECT * FROM conversion_batch {order} LIMIT ? OFFSET ?");
                sqlx::query_as::<_, ConversionBatch>(&sql)
                    .bind(params.limit)
                    .bind(params.offset)
                    .fetch_all(&self.executor)
                    .await?
            }
        };
        let total: u64 = match created_by {
            Some(user) => {
                sqlx::query_scalar("SELECT COUNT(*) FROM conversion_batch WHERE created_by = ?")
                    .bind(user)
                    .fetch_one(&self.executor)
                    .await?
            }
            None => {
                sqlx::query_scalar("SELECT COUNT(*) FROM conversion_batch")
                    .fetch_one(&self.executor)
                    .await?
            }
        };
        Ok(Batch {
            rows,
            total,
            offset: params.offset,
            limit: params.limit,
        })
    }
}
