use crate::{ChosenRow, FromRowPrefixed, language::LanguageShort, series::SeriesShort};
use serde::Serialize;
use sqlx::Row;

// #[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow, Repository)]
// pub struct Ebook {
//     #[spec(id)]
//     pub id: i64,

//     #[garde(length(min = 1, max = 511))]
//     pub title: String,

//     #[garde(length(min = 1, max = 5000))]
//     #[omit(short, sort)]
//     pub description: Option<String>,

//     #[garde(length(min = 1, max = 1023))]
//     pub cover: Option<String>,

//     pub base_dir: String,

//     #[garde(range(min = 0))]
//     pub series_id: Option<i64>,
//     pub series_index: Option<u32>,

//     #[garde(range(min = 0))]
//     pub language_id: Option<i64>,

//     #[garde(range(min = 0))]
//     #[spec(version)]
//     pub version: i64,
//     #[spec(created_by)]
//     pub created_by: Option<String>,
//     #[spec(created)]
//     pub created: time::PrimitiveDateTime,
//     #[spec(modified)]
//     pub modified: time::PrimitiveDateTime,
// }

#[derive(Debug, Serialize, Clone)]
pub struct Ebook {
    pub id: i64,

    pub title: String,

    pub description: Option<String>,

    pub cover: Option<String>,

    pub base_dir: String,

    pub series: Option<SeriesShort>,
    pub series_index: Option<u32>,

    pub language: LanguageShort,

    pub version: i64,
    pub created_by: Option<String>,
    pub created: time::PrimitiveDateTime,
    pub modified: time::PrimitiveDateTime,
}

impl sqlx::FromRow<'_, ChosenRow> for Ebook {
    fn from_row(row: &ChosenRow) -> Result<Self, sqlx::Error> {
        let language = LanguageShort::from_row_prefixed(row)?;
        let series = if row.try_get::<Option<i64>, _>("series_id")?.is_some() {
            Some(SeriesShort::from_row_prefixed(row)?)
        } else {
            None
        };
        Ok(Ebook {
            id: row.try_get("id")?,
            title: row.try_get("title")?,
            description: row.try_get("description")?,
            cover: row.try_get("cover")?,
            base_dir: row.try_get("base_dir")?,
            series,
            series_index: row.try_get("series_index")?,
            language,
            version: row.try_get("version")?,
            created_by: row.try_get("created_by")?,
            created: row.try_get("created")?,
            modified: row.try_get("modified")?,
        })
    }
}

pub struct EbookRepository<E> {
    executor: E,
}

impl<'c, E> EbookRepository<E>
where
    for<'a> &'a E: sqlx::Executor<'c, Database = crate::ChosenDB>, // + sqlx::Acquire<'c, Database = crate::ChosenDB>,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub async fn get(&self, id: i64) -> crate::error::Result<Ebook> {
        const SQL: &str = r#"
        SELECT e.id, e.title, e.description, e.cover, e.base_dir, e.series_id, e.series_index, e.language_id, e.version, 
        e.created_by, e.created, e.modified,
        l.code as language_code, l.name as language_name,
        s.title as series_title
        FROM ebook e 
        LEFT JOIN language l ON e.language_id = l.id
        LEFT JOIN series s ON e.series_id = s.id
        WHERE e.id = ?;
        "#;
        let record = sqlx::query_as::<_, Ebook>(SQL)
            .bind(id)
            .fetch_one(&self.executor)
            .await?
            .into();
        Ok(record)
    }
}
