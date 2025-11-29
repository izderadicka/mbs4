use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow, Repository)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Conversion {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 1023))]
    pub location: String,
    pub source_id: i64,
    pub format_id: i64,
    pub batch_id: Option<i64>,

    #[spec(created_by)]
    pub created_by: Option<String>,
    #[spec(created)]
    pub created: time::PrimitiveDateTime,
}

#[derive(Debug, Serialize, Clone, sqlx::FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EbookConversion {
    pub id: i64,
    pub location: String,
    pub source_id: i64,
    pub batch_id: Option<i64>,
    pub source_format_name: String,
    pub source_format_extension: String,
    pub format_name: String,
    pub format_extension: String,
    pub created_by: Option<String>,
    pub created: time::PrimitiveDateTime,
}

impl<'c, E> ConversionRepositoryImpl<E>
where
    for<'a> &'a E: sqlx::Executor<'c, Database = crate::ChosenDB>
        + sqlx::Acquire<'c, Database = crate::ChosenDB>,
{
    pub async fn list_for_ebook(
        &self,
        ebook_id: i64,
    ) -> crate::error::Result<Vec<EbookConversion>> {
        let sql = "SELECT c.id id, c.location location, c.source_id source_id, c.batch_id batch_id,
c.created created, c.created_by created_by,
f.name format_name, f.extension format_extension,
sf.name source_format_name, sf.extension source_format_extension
from conversion c
join format f on c.format_id = f.id
join source s on c.source_id = s.id
join format sf on  s.format_id = sf.id
where s.ebook_id = ? order by c.created desc limit 1000";
        let res = sqlx::query_as(sql)
            .bind(ebook_id)
            .fetch_all(&self.executor)
            .await?;
        Ok(res)
    }
}
