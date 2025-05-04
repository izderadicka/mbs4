use crate::{
    ChosenRow, FromRowPrefixed, author::AuthorShort, genre::GenreShort, language::LanguageShort,
    series::SeriesShort,
};
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

    pub authors: Option<Vec<AuthorShort>>,
    pub genres: Option<Vec<GenreShort>>,

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
            authors: None,
            genres: None,
        })
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct EbookShort {
    pub id: i64,
    pub title: String,
    pub has_cover: bool,
    pub series: Option<SeriesShort>,
    pub series_index: Option<u32>,
    pub language: LanguageShort,
    pub authors: Option<Vec<AuthorShort>>,
}

impl sqlx::FromRow<'_, ChosenRow> for EbookShort {
    fn from_row(row: &ChosenRow) -> Result<Self, sqlx::Error> {
        let language = LanguageShort::from_row_prefixed(row)?;
        let series = if row.try_get::<Option<i64>, _>("series_id")?.is_some() {
            Some(SeriesShort::from_row_prefixed(row)?)
        } else {
            None
        };

        Ok(EbookShort {
            id: row.try_get("id")?,
            title: row.try_get("title")?,
            has_cover: row.try_get::<Option<String>, _>("cover")?.is_some(),
            series,
            series_index: row.try_get("series_index")?,
            language,
            authors: None,
        })
    }
}

pub struct EbookRepository<E> {
    executor: E,
}

const VALID_ORDER_FIELDS: &[&str] = &["e.title", "series_index", "created", "modified"];

impl<'c, E> EbookRepository<E>
where
    for<'a> &'a E: sqlx::Executor<'c, Database = crate::ChosenDB>, // + sqlx::Acquire<'c, Database = crate::ChosenDB>,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub async fn list(
        &self,
        params: crate::ListingParams,
    ) -> crate::error::Result<Vec<EbookShort>> {
        let order = params.ordering(VALID_ORDER_FIELDS)?;
        let sql = format!(
            r#"
        SELECT e.id, e.title, e.cover,  e.series_id, e.series_index, e.language_id, 
        l.code as language_code, l.name as language_name,
        s.title as series_title
        FROM ebook e 
        LEFT JOIN language l ON e.language_id = l.id
        LEFT JOIN series s ON e.series_id = s.id
        {order}
        LIMIT ? OFFSET ?;
        "#
        );

        let mut res = sqlx::query_as::<_, EbookShort>(&sql)
            .bind(params.limit)
            .bind(params.offset)
            .fetch_all(&self.executor)
            .await?;

        // let ids = res.iter().map(|e| e.id).collect::<Vec<_>>();

        // Get authors more efficiently??? Does it make sense to do that?
        // select ebook_id,
        //     author_id,
        //     first_name,
        //     last_name
        // from (
        //         select row_number() OVER (
        //                 PARTITION BY ebook_id
        //                 ORDER BY author_id
        //             ) as rn,
        //             ebook_id,
        //             author_id,
        //             a.first_name,
        //             a.last_name
        //         from ebook_authors ea
        //             join author a on ea.author_id = a.id
        //         where ebook_id IN (72206, 79190, 80217)
        //     ) q
        // where rn <= 3;

        for ebook in res.iter_mut() {
            ebook.authors = Some(
                sqlx::query_as(
                    r#"
                SELECT a.id, a.first_name, a.last_name from author a 
                JOIN ebook_authors ea ON a.id = ea.author_id
                WHERE ea.ebook_id = ?
                LIMIT 3;
                "#,
                )
                .bind(ebook.id)
                .fetch_all(&self.executor)
                .await?,
            );
        }

        Ok(res)
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
        let mut record: Ebook = sqlx::query_as::<_, Ebook>(SQL)
            .bind(id)
            .fetch_one(&self.executor)
            .await?
            .into();
        record.authors = Some(
            sqlx::query_as(
                r#"
            SELECT a.id, a.first_name, a.last_name from author a 
            JOIN ebook_authors ea ON a.id = ea.author_id
            WHERE ea.ebook_id = ?
            ORDER BY a.last_name, a.first_name;
            "#,
            )
            .bind(id)
            .fetch_all(&self.executor)
            .await?,
        );
        record.genres = Some(
            sqlx::query_as(
                r#"
            SELECT g.id, g.name from genre g 
            JOIN ebook_genres eg ON g.id = eg.genre_id
            WHERE eg.ebook_id = ? ORDER BY g.name;
            "#,
            )
            .bind(id)
            .fetch_all(&self.executor)
            .await?,
        );

        Ok(record)
    }
}
