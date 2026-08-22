use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};
use sqlx::{Acquire, Executor};

use crate::{Batch, ChosenDB, error::Result};

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow, Repository)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EbookRating {
    #[spec(id)]
    pub id: i64,
    pub ebook_id: i64,

    #[garde(range(min = 0.0, max = 100.0))]
    pub rating: Option<f32>,
    #[garde(length(min = 1, max = 1000))]
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

impl<'c, E> EbookRatingRepositoryImpl<E>
where
    for<'a> &'a E: Executor<'c, Database = ChosenDB> + Acquire<'c, Database = ChosenDB>,
{
    pub async fn list_for_ebook(
        &self,
        ebook_id: i64,
        params: crate::ListingParams,
    ) -> Result<Batch<EbookRating>> {
        let rows: Vec<EbookRating> = sqlx::query_as(
            "SELECT * FROM ebook_rating WHERE ebook_id = ? ORDER BY created DESC LIMIT ? OFFSET ?",
        )
        .bind(ebook_id)
        .bind(params.limit)
        .bind(params.offset)
        .fetch_all(&self.executor)
        .await?;

        let count: u64 = sqlx::query_scalar("SELECT count(*) FROM ebook_rating WHERE ebook_id = ?")
            .bind(ebook_id)
            .fetch_one(&self.executor)
            .await?;

        Ok(Batch {
            offset: params.offset,
            limit: params.limit,
            rows,
            total: count,
        })
    }
}
