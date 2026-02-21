use crate::{
    Batch, ChosenDB, ChosenRow, Error, Filter, FromRowPrefixed, ListingParams, author::AuthorShort,
    genre::GenreShort, language::LanguageShort, now, series::SeriesShort,
};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use sqlx::{
    Acquire, Database, Executor, FromRow, Row,
    query::{Query, QueryAs},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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

    pub rating: Option<f32>,
    pub rating_count: Option<u32>,

    pub version: i64,
    pub created_by: Option<String>,
    pub created: time::PrimitiveDateTime,
    pub modified: time::PrimitiveDateTime,
}

impl Ebook {
    pub fn naming_meta(&self) -> mbs4_types::utils::naming::Ebook<'_> {
        use mbs4_types::utils::naming::Author;
        mbs4_types::utils::naming::Ebook {
            title: &self.title,
            authors: self
                .authors
                .as_ref()
                .map(|authors| {
                    authors
                        .iter()
                        .map(|author| Author {
                            first_name: author.first_name.as_ref().map(|s| s.as_str()),
                            last_name: author.last_name.as_str(),
                        })
                        .collect()
                })
                .unwrap_or_else(|| vec![]),
            language_code: self.language.code.as_str(),
            series_name: self.series.as_ref().map(|s| s.title.as_str()),
            series_index: self.series_index,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[garde(allow_unvalidated)]
pub struct CreateEbook {
    #[garde(length(min = 1, max = 255))]
    pub title: String,
    #[garde(length(min = 1, max = 5000))]
    pub description: Option<String>,

    pub series_id: Option<i64>,
    #[garde(range(min = 0))]
    pub series_index: Option<u32>,
    pub language_id: i64,

    pub authors: Option<Vec<i64>>,
    pub genres: Option<Vec<i64>>,

    pub created_by: Option<String>,
}

#[derive(Debug, Deserialize, Clone, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[garde(allow_unvalidated)]
pub struct UpdateEbook {
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    pub title: String,
    #[garde(length(min = 1, max = 5000))]
    pub description: Option<String>,
    #[garde(length(min = 1, max = 511))]
    pub cover: Option<String>,

    pub series_id: Option<i64>,
    #[garde(range(min = 0))]
    pub series_index: Option<u32>,
    pub language_id: i64,

    pub authors: Option<Vec<i64>>,
    pub genres: Option<Vec<i64>>,
    #[garde(range(min = 0))]
    pub version: i64,
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
            rating: row.try_get("rating")?,
            rating_count: row.try_get("rating_count")?,
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EbookShort {
    pub id: i64,
    pub title: String,
    pub has_cover: bool,
    pub series: Option<SeriesShort>,
    pub series_index: Option<u32>,
    pub language: LanguageShort,
    pub authors: Option<Vec<AuthorShort>>,
    pub rating: Option<f32>,
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
            rating: row.try_get("rating")?,
        })
    }
}

pub fn genres_filter(filters: Option<&Vec<Filter>>) -> Option<&Vec<i64>> {
    filters.as_ref().and_then(|f| {
        f.iter().find_map(|f| match f {
            Filter::Genres(genres) => Some(genres),
            #[allow(unreachable_patterns)]
            _ => None,
        })
    })
}

enum QueryType {
    Count,
    List,
    ListIds,
}

impl QueryType {
    const COUNT_QUERY: &'static str = "SELECT COUNT(*) FROM (SELECT e.id FROM ebook e ";
    const LIST_IDS_QUERY: &'static str = "SELECT e.id FROM ebook e ";
    const LIST_QUERY: &'static str = r#"
        SELECT e.id, e.title, e.cover,  e.series_id, e.series_index, e.language_id, e.rating,
        l.code as language_code, l.name as language_name,
        s.title as series_title
        FROM ebook e 
        LEFT JOIN language l ON e.language_id = l.id
        LEFT JOIN series s ON e.series_id = s.id
    "#;
    fn sql(&self) -> &'static str {
        match self {
            QueryType::Count => QueryType::COUNT_QUERY,
            QueryType::List => QueryType::LIST_QUERY,
            QueryType::ListIds => QueryType::LIST_IDS_QUERY,
        }
    }
}

struct EbookQuery<'a> {
    builder: sqlx::query_builder::QueryBuilder<'static, ChosenDB>,
    where_clause: Option<&'a Where>,
    limits: Option<&'a ListingParams>,
    filter: Option<&'a Vec<Filter>>,
    subquery: bool,
}

impl<'a> EbookQuery<'a> {
    fn new(
        query_type: QueryType,
        where_clause: Option<&'a Where>,
        limits: Option<&'a ListingParams>,
    ) -> Self {
        let builder = sqlx::query_builder::QueryBuilder::new(query_type.sql());
        EbookQuery {
            builder,
            where_clause,
            filter: limits.and_then(|l| l.filter.as_ref()),
            limits,
            subquery: false,
        }
    }

    fn new_count_query(where_clause: Option<&'a Where>, filter: Option<&'a Vec<Filter>>) -> Self {
        let builder = sqlx::query_builder::QueryBuilder::new(QueryType::Count.sql());
        EbookQuery {
            builder,
            where_clause,
            filter,
            limits: None,
            subquery: true,
        }
    }

    fn _build(&mut self) -> crate::error::Result<()> {
        if let Some(where_clause) = self.where_clause {
            if where_clause.author_id.is_some() {
                self.builder
                    .push(" JOIN ebook_authors ea ON e.id = ea.ebook_id ");
            }
        }

        if genres_filter(self.filter)
            .map(|g| !g.is_empty())
            .unwrap_or(false)
        {
            self.builder
                .push(" JOIN ebook_genres eg ON e.id = eg.ebook_id ");
        }

        let mut has_where = false;
        let mut push_expr = |expr, value| {
            if !has_where {
                self.builder.push(" WHERE ");
                has_where = true;
            } else {
                self.builder.push(" AND ");
            }
            self.builder.push(expr);
            if let Some(value) = value {
                self.builder.push_bind(value);
            }
        };

        if let Some(where_clause) = self.where_clause {
            if let Some(author_id) = where_clause.author_id {
                push_expr(" author_id = ", Some(author_id))
            }

            if let Some(series_id) = where_clause.series_id {
                push_expr(" series_id = ", Some(series_id))
            }
        }

        if let Some(genres_filter) = genres_filter(self.filter) {
            if !genres_filter.is_empty() {
                push_expr(" eg.genre_id IN (", None);
                let mut sep_builder = self.builder.separated(", ");
                for genre_id in genres_filter {
                    sep_builder.push_bind(*genre_id);
                }
                self.builder.push(") ");
            }

            self.builder
                .push(" GROUP BY e.id HAVING COUNT(DISTINCT eg.genre_id) = ");
            self.builder.push_bind(genres_filter.len() as i64);
            self.builder.push(" ");
        }

        if let Some(limits) = self.limits {
            if limits.order.is_some() {
                let order = limits.ordering(VALID_ORDER_FIELDS)?;
                self.builder.push(order);
                self.builder.push(" ");
            }

            self.builder.push("LIMIT ");
            self.builder.push_bind(limits.limit);
            self.builder.push(" OFFSET ");
            self.builder.push_bind(limits.offset);
        }

        if self.subquery {
            self.builder.push(" ) as sub");
        }

        Ok(())
    }

    #[allow(dead_code)]
    fn build_query(
        &mut self,
    ) -> crate::error::Result<Query<'_, ChosenDB, <ChosenDB as Database>::Arguments<'static>>> {
        self._build()?;
        Ok(self.builder.build())
    }

    fn build_query_as<'q, T>(
        &'q mut self,
    ) -> crate::error::Result<QueryAs<'q, ChosenDB, T, <ChosenDB as Database>::Arguments<'static>>>
    where
        T: FromRow<'q, <ChosenDB as Database>::Row>,
    {
        self._build()?;
        Ok(self.builder.build_query_as())
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
}

pub struct EbookRepositoryImpl<E> {
    executor: E,
}

const VALID_ORDER_FIELDS: &[&str] = &[
    "e.title",
    "s.title",
    "e.series_index",
    "e.created",
    "e.modified",
    "e.id",
];

pub type EbookRepository = EbookRepositoryImpl<sqlx::Pool<ChosenDB>>;

impl<'c, E> EbookRepositoryImpl<E>
where
    for<'a> &'a E: Executor<'c, Database = ChosenDB> + Acquire<'c, Database = ChosenDB>,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub async fn count(&self) -> crate::error::Result<u64> {
        self._count(None, None).await
    }

    async fn _count(
        &self,
        where_clause: Option<&Where>,
        filter: Option<&Vec<Filter>>,
    ) -> crate::error::Result<u64> {
        let mut builder = EbookQuery::new_count_query(where_clause, filter);
        let query = builder.build_query_as::<(u64,)>()?;
        let res = query.fetch_one(&self.executor).await?;
        Ok(res.0)
    }

    pub async fn list(
        &self,
        params: crate::ListingParams,
    ) -> crate::error::Result<Batch<EbookShort>> {
        self._list(params, None).await
    }

    pub async fn map_ids_to_ebooks(&self, ids: &[i64]) -> crate::error::Result<Vec<Ebook>> {
        let mut ebooks = Vec::with_capacity(ids.len());
        for id in ids {
            ebooks.push(self.get(*id).await?);
        }
        Ok(ebooks)
    }

    pub async fn map_short_to_ebooks(
        &self,
        shorts: &[EbookShort],
    ) -> crate::error::Result<Vec<Ebook>> {
        let mut ebooks = Vec::with_capacity(shorts.len());
        for short in shorts {
            ebooks.push(self.get(short.id).await?);
        }
        Ok(ebooks)
    }

    pub async fn list_ids(&self, params: crate::ListingParams) -> crate::error::Result<Batch<i64>> {
        self._list_ids(params, None).await
    }

    pub async fn list_all(&self) -> crate::error::Result<Vec<EbookShort>> {
        self.list(crate::ListingParams::new_unpaged())
            .await
            .map(|b| b.rows)
    }

    pub async fn list_by_author(
        &self,
        params: crate::ListingParams,
        author_id: i64,
    ) -> crate::error::Result<Batch<EbookShort>> {
        self._list(params, Some(Where::new().author(author_id)))
            .await
    }

    pub async fn list_ids_by_author(
        &self,
        params: crate::ListingParams,
        author_id: i64,
    ) -> crate::error::Result<Batch<i64>> {
        self._list_ids(params, Some(Where::new().author(author_id)))
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

    pub async fn list_ids_by_series(
        &self,
        params: crate::ListingParams,
        series_id: i64,
    ) -> crate::error::Result<Batch<i64>> {
        self._list_ids(params, Some(Where::new().series(series_id)))
            .await
    }

    async fn _list_ids(
        &self,
        params: crate::ListingParams,
        where_clause: Option<Where>,
    ) -> crate::error::Result<Batch<i64>> {
        let mut builder = EbookQuery::new(QueryType::ListIds, where_clause.as_ref(), Some(&params));
        let query = builder.build_query_as::<(i64,)>()?;

        let count = self
            ._count(where_clause.as_ref(), params.filter.as_ref())
            .await?;

        let res = query.fetch_all(&self.executor).await?;
        Ok(Batch {
            rows: res.iter().map(|r| r.0).collect(),
            total: count,
            offset: params.offset,
            limit: params.limit,
        })
    }

    async fn _list(
        &self,
        params: crate::ListingParams,
        where_clause: Option<Where>,
    ) -> crate::error::Result<Batch<EbookShort>> {
        let mut builder = EbookQuery::new(QueryType::List, where_clause.as_ref(), Some(&params));

        let query = builder.build_query_as::<EbookShort>()?;

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

        let count = self
            ._count(where_clause.as_ref(), params.filter.as_ref())
            .await?;

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
        e.rating, e.rating_count,
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

    pub async fn create(&self, payload: CreateEbook) -> crate::error::Result<Ebook> {
        match (&payload.series_id, &payload.series_index) {
            (Some(_), None) | (None, Some(_)) => {
                return Err(Error::InvalidEntity(
                    "Series name and index must be provided together".into(),
                ));
            }
            _ => (),
        }

        let mut transaction = self.executor.begin().await?;

        let lang_code: String = sqlx::query_scalar("SELECT code FROM language WHERE id = ?")
            .bind(payload.language_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(Error::DBReferenceError)?;
        let series_name: Option<String> = if let Some(series_id) = payload.series_id {
            sqlx::query_scalar("SELECT title FROM series WHERE id = ?")
                .bind(series_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(Error::DBReferenceError)?
        } else {
            None
        };

        struct ShortAuthor {
            first_name: Option<String>,
            last_name: String,
        }

        let authors = if let Some(author_ids) = payload.authors.as_ref() {
            if !author_ids.is_empty() {
                let mut authors = Vec::with_capacity(author_ids.len());
                let placeholders = author_ids
                    .iter()
                    .map(|_| "?")
                    .collect::<Vec<_>>()
                    .join(", ");

                let query = format!(
                    "SELECT first_name, last_name FROM author WHERE id IN ({placeholders})"
                );
                let mut query = sqlx::query(&query);
                for author_id in author_ids.iter() {
                    query = query.bind(author_id);
                }
                let mut stream = query.fetch(&mut *transaction);
                while let Some(res) = stream.next().await {
                    let row = res?;
                    authors.push(ShortAuthor {
                        first_name: row.get(0),
                        last_name: row.get(1),
                    })
                }
                authors
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        if authors.len() != payload.authors.as_ref().map(|a| a.len()).unwrap_or(0) {
            return Err(Error::InvalidEntity(
                "Some authors could not be found".to_string(),
            ));
        }

        let book_meta = mbs4_types::utils::naming::Ebook {
            title: &payload.title,
            authors: authors
                .iter()
                .map(|a| mbs4_types::utils::naming::Author {
                    first_name: a.first_name.as_deref(),
                    last_name: &a.last_name,
                })
                .collect(),
            language_code: &lang_code,
            series_name: series_name.as_deref(),
            series_index: payload.series_index,
        };

        let base_dir = book_meta
            .ebook_base_dir()
            .ok_or_else(|| Error::InvalidEntity("Cannot construct base dir".to_string()))?;

        let now = now();
        let query = "INSERT INTO ebook (title, description, base_dir, series_id, series_index, language_id, version, created_by, created, modified) 
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?);";

        let book_id = sqlx::query(query)
            .bind(payload.title)
            .bind(payload.description)
            .bind(base_dir)
            .bind(payload.series_id)
            .bind(payload.series_index)
            .bind(payload.language_id)
            .bind(1)
            .bind(payload.created_by)
            .bind(now)
            .bind(now)
            .execute(&mut *transaction)
            .await?
            .last_insert_rowid();

        insert_ebook_dependencies(book_id, payload.genres, payload.authors, &mut transaction)
            .await?;

        transaction.commit().await?;

        self.get(book_id).await
    }

    pub async fn update(&self, id: i64, payload: UpdateEbook) -> crate::error::Result<Ebook> {
        match (&payload.series_id, &payload.series_index) {
            (Some(_), None) | (None, Some(_)) => {
                return Err(Error::InvalidEntity(
                    "Series name and index must be provided together".into(),
                ));
            }
            _ => (),
        }

        if payload.id != id {
            return Err(crate::Error::InvalidEntity("Entity id mismatch".into()));
        }

        let mut transaction = self.executor.begin().await?;

        let sql = "UPDATE ebook SET title = ?, description = ?, series_id = ?, series_index = ?, language_id = ?, modified = ?, version = version + 1 WHERE id = ? AND version = ?";
        let num_update = sqlx::query(sql)
            .bind(payload.title)
            .bind(payload.description)
            .bind(payload.series_id)
            .bind(payload.series_index)
            .bind(payload.language_id)
            .bind(now())
            .bind(id)
            .bind(payload.version)
            .execute(&mut *transaction)
            .await?
            .rows_affected();

        if num_update == 0 {
            return Err(Error::RecordNotFound("Ebook".to_string()));
        }

        sqlx::query("DELETE FROM ebook_authors WHERE ebook_id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;

        sqlx::query("DELETE FROM ebook_genres WHERE ebook_id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;

        insert_ebook_dependencies(id, payload.genres, payload.authors, &mut transaction).await?;

        transaction.commit().await?;
        self.get(id).await
    }

    pub async fn delete(&self, id: i64) -> crate::error::Result<()> {
        let res = sqlx::query("DELETE FROM ebook WHERE id = ?")
            .bind(id)
            .execute(&self.executor)
            .await?;

        if res.rows_affected() == 0 {
            Err(crate::error::Error::RecordNotFound("Language".to_string()))
        } else {
            Ok(())
        }
    }

    pub async fn update_cover(
        &self,
        id: i64,
        cover: Option<String>,
        ebook_version: i64,
    ) -> crate::error::Result<Ebook> {
        let sql = "UPDATE ebook SET cover = ?, modified = ?, version = version + 1 WHERE id = ? and version = ?";
        let res = sqlx::query(sql)
            .bind(cover)
            .bind(now())
            .bind(id)
            .bind(ebook_version)
            .execute(&self.executor)
            .await?;
        if res.rows_affected() == 0 {
            return Err(Error::RecordNotFound("Ebook".to_string()));
        }
        let ebook = self.get(id).await?;
        Ok(ebook)
    }

    pub async fn merge(&self, from_id: i64, to_id: i64) -> crate::error::Result<()> {
        let mut tx = self.executor.begin().await?;

        sqlx::query("UPDATE source SET ebook_id = ? WHERE ebook_id = ?")
            .bind(to_id)
            .bind(from_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM ebook WHERE id = ?")
            .bind(from_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        Ok(())
    }

    pub async fn rate(
        &self,
        id: i64,
        rating: f32,
        user: String,
        description: Option<String>,
    ) -> crate::error::Result<Ebook> {
        let mut tx = self.executor.begin().await?;
        let ebook_version: i64 = sqlx::query_scalar("SELECT version FROM ebook WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        let timestamp = now();
        let existing: Option<(i64, i64)> = sqlx::query_as(
            "SELECT id, version FROM ebook_rating WHERE ebook_id = ? AND created_by = ?",
        )
        .bind(id)
        .bind(user.clone())
        .fetch_one(&mut *tx)
        .await
        .ok();

        if let Some((existing_id, existing_version)) = existing {
            let res = if let Some(description) = description {
                sqlx::query("UPDATE ebook_rating SET rating = ?, description = ?, modified = ?, version = version + 1 WHERE id = ? and version = ?")
                    .bind(rating)
                    .bind(description)
                    .bind(timestamp)
                    .bind(existing_id)
                    .bind(existing_version)
                    .execute(&mut *tx)
                    .await?
            } else {
                sqlx::query("UPDATE ebook_rating SET rating = ?, modified = ?, version = version + 1 WHERE id = ? and version = ?")
                    .bind(rating)
                    .bind(timestamp)
                    .bind(existing_id)
                    .bind(existing_version)
                    .execute(&mut *tx)
                    .await?
            };
            if res.rows_affected() == 0 {
                return Err(Error::RecordNotFound("Ebook rating".to_string()));
            }
        } else {
            sqlx::query(
                "INSERT INTO ebook_rating (ebook_id, created_by, rating, description, created, modified, version) VALUES (?, ?, ?, ?, ?, ?, 1)",
            )
            .bind(id)
            .bind(user)
            .bind(rating)
            .bind(description)
            .bind(timestamp)
            .bind(timestamp)
            .execute(&mut *tx)
            .await?;
        }

        let (new_rating, num_ratings): (f32, i64) = sqlx::query_as(
            "SELECT AVG(rating), COUNT(rating) FROM ebook_rating WHERE ebook_id = ?",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE ebook SET rating = ?, rating_count = ?, modified = ?, version = version + 1 WHERE id = ? and version = ?")
            .bind(new_rating)
            .bind(num_ratings)
            .bind(now())
            .bind(id)
            .bind(ebook_version)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        self.get(id).await
    }
}

async fn insert_ebook_dependencies(
    book_id: i64,
    genres: Option<Vec<i64>>,
    authors: Option<Vec<i64>>,
    transaction: &mut sqlx::Transaction<'_, ChosenDB>,
) -> crate::error::Result<()> {
    if let Some(ref genres) = genres {
        let query = "INSERT INTO ebook_genres (ebook_id, genre_id) VALUES (?, ?);";
        for genre_id in genres.iter() {
            sqlx::query(query)
                .bind(book_id)
                .bind(genre_id)
                .execute(&mut **transaction)
                .await
                .map_err(Error::DBReferenceError)?;
        }
    }

    if let Some(ref authors) = authors {
        let query = "INSERT INTO ebook_authors (ebook_id, author_id) VALUES (?, ?);";
        for author_id in authors.iter() {
            sqlx::query(query)
                .bind(book_id)
                .bind(author_id)
                .execute(&mut **transaction)
                .await
                .map_err(Error::DBReferenceError)?;
        }
    }

    Ok(())
}
