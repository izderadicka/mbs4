use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow, Repository)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Format {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    pub name: String,
    #[garde(length(min = 3, max = 255))]
    pub mime_type: String,
    #[garde(length(min = 1, max = 32))]
    pub extension: String,
    #[garde(range(min = 0))]
    #[spec(version)]
    pub version: i64,
}

impl<'c, E> FormatRepositoryImpl<E>
where
    for<'a> &'a E: sqlx::Executor<'c, Database = crate::ChosenDB>
        + sqlx::Acquire<'c, Database = crate::ChosenDB>,
{
    pub async fn get_by_mime_type(&self, mime_type: &str) -> crate::error::Result<Format> {
        let record = sqlx::query_as::<_, Format>("SELECT * FROM format WHERE mime_type = $1")
            .bind(mime_type)
            .fetch_one(&self.executor)
            .await?;
        Ok(record)
    }

    pub async fn get_by_extension(&self, extension: &str) -> crate::error::Result<Format> {
        let record = sqlx::query_as::<_, Format>("SELECT * FROM format WHERE extension = $1")
            .bind(extension)
            .fetch_one(&self.executor)
            .await?;
        Ok(record)
    }
}
