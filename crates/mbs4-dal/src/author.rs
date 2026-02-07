use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};
use sqlx::{Acquire, Executor};

use crate::ChosenDB;

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow, Repository)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Author {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    pub last_name: String,
    #[garde(length(min = 1, max = 255))]
    pub first_name: Option<String>,
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

impl<'c, E> AuthorRepositoryImpl<E>
where
    for<'a> &'a E: Executor<'c, Database = ChosenDB> + Acquire<'c, Database = ChosenDB>,
{
    pub async fn merge(&self, from_id: i64, to_id: i64) -> crate::error::Result<()> {
        let mut tx = self.executor.begin().await?;

        sqlx::query("UPDATE ebook_authors SET author_id = ? WHERE author_id = ?")
            .bind(to_id)
            .bind(from_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM author WHERE id = ?")
            .bind(from_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        Ok(())
    }
}
