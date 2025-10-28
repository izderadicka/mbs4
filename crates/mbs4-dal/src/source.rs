use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow, Repository)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Source {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 1023))]
    pub location: String,
    pub ebook_id: i64,
    pub format_id: i64,
    #[garde(range(min = 1))]
    pub size: i64,
    #[garde(length(min = 20, max = 64))]
    pub hash: String,
    #[garde(range(min = 0.0, max = 100.0))]
    pub quality: Option<f32>,
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

#[derive(Debug, Serialize, Clone, sqlx::FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EbookSource {
    pub id: i64,
    pub location: String,
    pub format_name: String,
    pub format_extension: String,
    pub size: i64,
    pub quality: Option<f32>,
    pub created_by: Option<String>,
    pub created: time::PrimitiveDateTime,
}

impl<'c, E> SourceRepositoryImpl<E>
where
    for<'a> &'a E: sqlx::Executor<'c, Database = crate::ChosenDB>
        + sqlx::Acquire<'c, Database = crate::ChosenDB>,
{
    pub async fn list_for_ebook(&self, ebook_id: i64) -> crate::error::Result<Vec<EbookSource>> {
        let sql = "SELECT s.id id, s.location location, f.name format_name, f.extension format_extension,  
s.size size, s.quality quality,
s.created created, s.created_by created_by
FROM source s join format f on s.format_id = f.id 
WHERE ebook_id = ? ORDER BY created DESC LIMIT 1000";
        let res = sqlx::query_as(sql)
            .bind(ebook_id)
            .fetch_all(&self.executor)
            .await?;
        Ok(res)
    }
}
