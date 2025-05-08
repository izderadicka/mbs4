use crate::{
    Batch, ChosenRow, FromRowPrefixed, author::AuthorShort, genre::GenreShort,
    language::LanguageShort, series::SeriesShort,
};
use serde::Serialize;
use sqlx::{Row, query::QueryAs};

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

#[derive(Default)]
struct Where {
    series_id: Option<i64>,
    author_id: Option<i64>,
}

impl Where {
    fn new() -> Self {
        Self::default()
    }

    fn author(mut self, author_id: i64) -> Self {
        self.author_id = Some(author_id);
        self
    }

    fn series(mut self, series_id: i64) -> Self {
        self.series_id = Some(series_id);
        self
    }

    fn bind<'q, DB, O, A>(&self, mut query: QueryAs<'q, DB, O, A>) -> QueryAs<'q, DB, O, A>
    where
        DB: sqlx::Database<Arguments<'q> = A>,
        i64: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    {
        if let Some(author_id) = self.author_id {
            query = query.bind(author_id)
        }
        if let Some(series_id) = self.series_id {
            query = query.bind(series_id)
        }
        query
    }

    fn where_clause(&self) -> Option<String> {
        let mut where_clause = Vec::new();
        if self.author_id.is_some() {
            where_clause.push("author_id = ?");
        }
        if self.series_id.is_some() {
            where_clause.push("series_id = ?");
        }

        if where_clause.is_empty() {
            None
        } else {
            Some(format!("WHERE {}", where_clause.join(" AND ")))
        }
    }

    fn extra_tables(&self) -> Option<String> {
        if self.author_id.is_some() {
            Some("JOIN ebook_authors ea ON e.id = ea.ebook_id ".to_string())
        } else {
            None
        }
    }
}

pub struct EbookRepositoryImpl<E> {
    executor: E,
}

const VALID_ORDER_FIELDS: &[&str] = &["e.title", "s.title", "series_index", "created", "modified"];

pub type EbookRepository = EbookRepositoryImpl<sqlx::Pool<crate::ChosenDB>>;

impl<'c, E> EbookRepositoryImpl<E>
where
    for<'a> &'a E: sqlx::Executor<'c, Database = crate::ChosenDB>, // + sqlx::Acquire<'c, Database = crate::ChosenDB>,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub async fn count(&self) -> crate::error::Result<u64> {
        self._count(&None).await
    }

    async fn _count(&self, where_clause: &Option<Where>) -> crate::error::Result<u64> {
        let sql = format!(
            "SELECT COUNT(*) FROM ebook e {extra_tables} {where_clause} ",
            extra_tables = where_clause
                .as_ref()
                .and_then(|w| w.extra_tables())
                .unwrap_or_default(),
            where_clause = where_clause
                .as_ref()
                .and_then(|w| w.where_clause())
                .unwrap_or_default(),
        );
        let mut query = sqlx::query_as::<_, (u64,)>(&sql);
        if let Some(w) = where_clause {
            query = w.bind(query);
        }
        let res = query.fetch_one(&self.executor).await?;
        Ok(res.0)
    }

    pub async fn list(
        &self,
        params: crate::ListingParams,
    ) -> crate::error::Result<Batch<EbookShort>> {
        self._list(params, None).await
    }

    pub async fn list_by_author(
        &self,
        params: crate::ListingParams,
        author_id: i64,
    ) -> crate::error::Result<Batch<EbookShort>> {
        self._list(params, Some(Where::new().author(author_id)))
            .await
    }

    pub async fn list_by_series(
        &self,
        params: crate::ListingParams,
        series_id: i64,
    ) -> crate::error::Result<Batch<EbookShort>> {
        self._list(params, Some(Where::new().series(series_id)))
            .await
    }

    async fn _list(
        &self,
        params: crate::ListingParams,
        where_clause: Option<Where>,
    ) -> crate::error::Result<Batch<EbookShort>> {
        let order = params.ordering(VALID_ORDER_FIELDS)?;
        let extra_tables = where_clause
            .as_ref()
            .and_then(Where::extra_tables)
            .unwrap_or_default();
        let where_cond = where_clause
            .as_ref()
            .and_then(Where::where_clause)
            .unwrap_or_default();
        let sql = format!(
            r#"
        SELECT e.id, e.title, e.cover,  e.series_id, e.series_index, e.language_id, 
        l.code as language_code, l.name as language_name,
        s.title as series_title
        FROM ebook e 
        LEFT JOIN language l ON e.language_id = l.id
        LEFT JOIN series s ON e.series_id = s.id
        {extra_tables}
        {where_cond}
        {order}
        LIMIT ? OFFSET ?;
        "#
        );

        // println!("SQL: {}", sql);

        let mut query = sqlx::query_as::<_, EbookShort>(&sql);

        if let Some(ref where_clause) = where_clause {
            query = where_clause.bind(query);
        }

        let mut res = query
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
                SELECT a.id, a.first_name, a.last_name FROM author a 
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

        let count = self._count(&where_clause).await?;

        Ok(Batch {
            offset: params.offset,
            limit: params.limit,
            rows: res,
            total: count,
        })
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
