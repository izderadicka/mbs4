use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};
use sqlx::{Acquire, Executor};

use crate::ChosenDB;

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow, Repository)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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

impl<'c, E> SeriesRepositoryImpl<E>
where
    for<'a> &'a E: Executor<'c, Database = ChosenDB> + Acquire<'c, Database = ChosenDB>,
{
    pub async fn merge(&self, from_id: i64, to_id: i64) -> crate::error::Result<()> {
        let mut tx = self.executor.begin().await?;

        sqlx::query("UPDATE ebook SET series_id = ? WHERE series_id = ?")
            .bind(to_id)
            .bind(from_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM series WHERE id = ?")
            .bind(from_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        Ok(())
    }
}
