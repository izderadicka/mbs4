use std::collections::HashMap;

use mbs4_macros::Repository;
use mbs4_types::utils::naming::Author;
use serde::{Deserialize, Serialize};
use sqlx::{
    Acquire, Database, Executor, FromRow, Row,
    query::{Query, QueryAs, QueryScalar},
};

use crate::{
    Batch, ChosenDB, ChosenRow, Error,
    author::AuthorShort,
    ebook::{self, EbookShort},
    error::Result,
    series::{self, SeriesShort},
};

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow, Repository)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Bookshelf {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    pub name: String,
    #[garde(length(min = 1, max = 5000))]
    #[omit(short, sort)]
    pub description: Option<String>,
    pub public: bool,
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
pub struct BookshelfListing {
    pub id: i64,
    pub name: String,
    pub items_count: u64,
    pub created_by: Option<String>,
    pub created: time::PrimitiveDateTime,
    pub modified: time::PrimitiveDateTime,
}

#[derive(Debug, Serialize, Clone)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BookshelfItemListing {
    pub id: i64,
    pub note: Option<String>,
    pub created: time::PrimitiveDateTime,
    pub modified: time::PrimitiveDateTime,
    pub item_type: String,
    pub ebook_id: Option<i64>,
    pub series_id: Option<i64>,
    pub title: String,
    pub has_cover: bool,
    pub authors: Option<Vec<AuthorShort>>,
    pub series_title: Option<String>,
    pub series_index: Option<i64>,
}

impl sqlx::FromRow<'_, ChosenRow> for BookshelfItemListing {
    fn from_row(row: &'_ ChosenRow) -> std::result::Result<Self, sqlx::Error> {
        let id = row.try_get("id")?;
        let note = row.try_get("note")?;
        let created = row.try_get("created")?;
        let modified = row.try_get("modified")?;
        let item_type = row.try_get("item_type")?;
        let ebook_id: Option<i64> = row.try_get("ebook_id")?;
        let series_id: Option<i64> = row.try_get("series_id")?;

        let ebook_cover: Option<String> = row.try_get("ebook_cover")?;
        let ebook_title: Option<String> = row.try_get("ebook_title")?;
        let series_title: Option<String> = row.try_get("series_title")?;
        let has_cover = ebook_cover.is_some();
        let ebook_series_title: Option<String> = row.try_get("ebook_series_title")?;
        let ebook_series_index: Option<i64> = row.try_get("ebook_series_index")?;
        let authors = None;
        Ok(BookshelfItemListing {
            id,
            note,
            created,
            modified,
            item_type,
            ebook_id,
            series_id,
            title: ebook_title.or(series_title).unwrap_or_default(),
            has_cover,
            authors,
            series_title: ebook_series_title,
            series_index: ebook_series_index,
        })
    }
}

const ITEM_VALID_ORDER_FIELDS: &[&str] = &["created", "modified", "order", "id"];

struct BookshelfQueryBuilder {
    builder: sqlx::query_builder::QueryBuilder<'static, ChosenDB>,
    is_count: bool,
}

impl BookshelfQueryBuilder {
    fn new() -> Self {
        let builder = sqlx::query_builder::QueryBuilder::new(
            "SELECT *, (select count(*) from bookshelf_item where bookshelf_id = bookshelf.id) as items_count FROM bookshelf ",
        );
        BookshelfQueryBuilder {
            builder,
            is_count: false,
        }
    }

    fn new_count_query() -> Self {
        let builder = sqlx::query_builder::QueryBuilder::new("SELECT COUNT(*) FROM bookshelf ");
        BookshelfQueryBuilder {
            builder,
            is_count: true,
        }
    }

    fn private(&mut self, user: impl Into<String>) -> &mut Self {
        self.builder.push(" WHERE created_by =  ");
        self.builder.push_bind(user.into());
        self
    }

    fn public(&mut self, user: impl Into<String>) -> &mut Self {
        self.builder
            .push(" WHERE public = true AND created_by !=  ");
        self.builder.push_bind(user.into());
        self
    }

    fn order_and_limit(&mut self, params: &crate::ListingParams) -> Result<&mut Self> {
        let order = params.ordering(VALID_ORDER_FIELDS)?;
        if !order.is_empty() {
            self.builder.push(format!(" {order} "));
        }
        self.builder.push(" LIMIT ");
        self.builder.push_bind(params.limit);
        self.builder.push(" OFFSET ");
        self.builder.push_bind(params.offset);

        Ok(self)
    }

    fn build_query_as<'q, T>(
        &'q mut self,
    ) -> QueryAs<'q, ChosenDB, T, <ChosenDB as Database>::Arguments<'static>>
    where
        T: FromRow<'q, <ChosenDB as Database>::Row>,
    {
        assert!(!self.is_count);
        self.builder.build_query_as()
    }

    fn build_count_query(
        &mut self,
    ) -> QueryScalar<'_, ChosenDB, u64, <ChosenDB as Database>::Arguments<'static>> {
        self.builder.build_query_scalar()
    }
}

struct SeriesAuthorsQueryBuilder {
    builder: sqlx::query_builder::QueryBuilder<'static, ChosenDB>,
    has_limit: bool,
}

impl SeriesAuthorsQueryBuilder {
    fn new() -> Self {
        let builder = sqlx::query_builder::QueryBuilder::new(
            "
SELECT x.series_id,
    a.id,
    a.first_name,
    a.last_name
FROM (
        SELECT series_id,
            author_id
        FROM (
                SELECT series_id,
                    author_id,
                    row_number() OVER (
                        PARTITION BY series_id
                        ORDER BY author_id
                    ) AS rn
                FROM (
                        SELECT DISTINCT e.series_id AS series_id,
                            ea.author_id AS author_id
                        FROM ebook_authors ea
                            JOIN ebook e ON e.id = ea.ebook_id",
        );
        SeriesAuthorsQueryBuilder {
            builder,
            has_limit: false,
        }
    }

    fn limit_for<I>(&mut self, ids: I)
    where
        I: IntoIterator<Item = i64>,
    {
        self.builder.push(" WHERE e.series_id IN ( ");
        let mut list = self.builder.separated(", ");
        for id in ids {
            list.push_bind(id);
            self.has_limit = true;
        }
        self.builder.push(" ) ");
    }

    fn _finish(&mut self) {
        self.builder.push(
            "
    ) d
            ) t
        WHERE rn <= 3
    ) x
    JOIN author a ON a.id = x.author_id;",
        );
    }

    fn build_query<'q>(
        &'q mut self,
    ) -> Query<'q, ChosenDB, <ChosenDB as Database>::Arguments<'static>> {
        self._finish();
        self.builder.build()
    }

    fn is_empty(&self) -> bool {
        !self.has_limit
    }
}

struct AuthorsQueryBuilder {
    builder: sqlx::query_builder::QueryBuilder<'static, ChosenDB>,
    has_limit: bool,
}

impl AuthorsQueryBuilder {
    fn new() -> Self {
        let builder = sqlx::query_builder::QueryBuilder::new(
            "
SELECT ebook_id,
    id,
    first_name,
    last_name
FROM (
        SELECT ea.ebook_id,
            a.id,
            a.first_name,
            a.last_name,
            row_number() OVER (
                PARTITION BY ea.ebook_id
                ORDER BY a.id
            ) AS rn
        FROM ebook_authors ea
            JOIN author a ON a.id = ea.author_id ",
        );
        AuthorsQueryBuilder {
            builder,
            has_limit: false,
        }
    }

    fn limit_for<I>(&mut self, ids: I)
    where
        I: IntoIterator<Item = i64>,
    {
        self.builder.push(" WHERE ea.ebook_id IN ( ");
        let mut list = self.builder.separated(", ");
        for id in ids {
            list.push_bind(id);
            self.has_limit = true;
        }
        self.builder.push(" ) ");
    }

    fn _finish(&mut self) {
        self.builder.push(
            "
    ) t
WHERE rn <= 3;",
        );
    }

    fn build_query<'q>(
        &'q mut self,
    ) -> Query<'q, ChosenDB, <ChosenDB as Database>::Arguments<'static>> {
        self._finish();
        self.builder.build()
    }

    fn is_empty(&self) -> bool {
        !self.has_limit
    }
}

impl<'c, E> BookshelfRepositoryImpl<E>
where
    for<'a> &'a E: Executor<'c, Database = ChosenDB> + Acquire<'c, Database = ChosenDB>,
{
    async fn list_owned(
        &self,
        user: &str,
        private: bool,
        params: crate::ListingParams,
    ) -> Result<Batch<BookshelfListing>> {
        let mut builder = BookshelfQueryBuilder::new();
        if private {
            builder.private(user);
        } else {
            builder.public(user);
        };
        let query = builder
            .order_and_limit(&params)?
            .build_query_as::<BookshelfListing>();
        let res = query
            .fetch(&self.executor)
            .take(crate::MAX_LIMIT)
            .try_collect::<Vec<_>>()
            .await?;
        let mut builder = BookshelfQueryBuilder::new_count_query();
        if private {
            builder.private(user);
        } else {
            builder.public(user);
        }

        let count_query = builder.build_count_query();
        let count = count_query.fetch_one(&self.executor).await?;
        let batch = Batch {
            rows: res,
            total: count,
            offset: params.offset,
            limit: params.limit,
        };
        Ok(batch)
    }

    // List users bookshelves - created by user
    pub async fn list_for_user(
        &self,
        user: &str,
        params: crate::ListingParams,
    ) -> Result<Batch<BookshelfListing>> {
        self.list_owned(user, true, params).await
    }

    // List others public bookshelves - created by other users and

    pub async fn list_public(
        &self,
        user: &str,
        params: crate::ListingParams,
    ) -> Result<Batch<BookshelfListing>> {
        self.list_owned(user, false, params).await
    }

    pub async fn get_owner(&self, id: i64) -> Result<String> {
        let user = sqlx::query_scalar("SELECT created_by FROM bookshelf WHERE id = ?")
            .bind(id)
            .fetch_one(&self.executor)
            .await?;
        Ok(user)
    }

    // List bookshelf items, paged, sorted - can list only public or mine
    pub async fn list_items(
        &self,
        bookshelf_id: i64,
        params: crate::ListingParams,
    ) -> Result<Batch<BookshelfItemListing>> {
        // Due to way how authors are fetch we do not allow big pages
        if params.limit > 1000 {
            return Err(Error::InvalidPageSize(
                "For bookshelf items max is 1000".into(),
            ));
        }
        let order = params.ordering(ITEM_VALID_ORDER_FIELDS)?;
        let query = format!(
            "
select i.id as id,
    i.note as note,
    i.created as created,
    i.modified as modified,
    i.type as item_type,
    i.ebook_id as ebook_id,
    i.series_id as series_id,
    e.title as ebook_title,
    e.cover as ebook_cover,
    e.series_index as ebook_series_index,
    s.title as series_title,
    ebs.title as ebook_series_title
from (
        select id,
            note,
            created,
            modified,
            type,
            ebook_id,
            series_id,
            \"order\"
        from bookshelf_item i
        where bookshelf_id = ?
        {order}
        limit ? offset ?
    ) i
    left join ebook e on i.ebook_id = e.id
    left join series s on i.series_id = s.id
    left join series ebs on e.series_id = ebs.id
{order};"
        );
        let mut res: Vec<BookshelfItemListing> = sqlx::query_as(&query)
            .bind(bookshelf_id)
            .bind(params.limit)
            .bind(params.offset)
            .fetch(&self.executor)
            .take(crate::MAX_LIMIT)
            .try_collect::<Vec<_>>()
            .await?;
        let count =
            sqlx::query_scalar("SELECT COUNT(*) FROM bookshelf_item WHERE bookshelf_id = ?")
                .bind(bookshelf_id)
                .fetch_one(&self.executor)
                .await?;

        let mut authors_query = AuthorsQueryBuilder::new();
        authors_query.limit_for(res.iter().filter_map(|e| e.ebook_id));
        let mut authors_map = HashMap::new();
        if !authors_query.is_empty() {
            let mut authors = authors_query.build_query().fetch(&self.executor);
            while let Some(author_row) = authors.try_next().await? {
                let ebook_id: i64 = author_row.try_get("ebook_id")?;
                let author = AuthorShort {
                    id: author_row.try_get("id")?,
                    first_name: author_row.try_get("first_name")?,
                    last_name: author_row.try_get("last_name")?,
                };
                authors_map
                    .entry(ebook_id)
                    .or_insert_with(Vec::new)
                    .push(author);
            }
        }

        let mut authors_query = SeriesAuthorsQueryBuilder::new();
        authors_query.limit_for(res.iter().filter_map(|e| e.series_id));
        let mut series_authors_map = HashMap::new();
        if !authors_query.is_empty() {
            let mut authors = authors_query.build_query().fetch(&self.executor);
            while let Some(author_row) = authors.try_next().await? {
                let series_id: i64 = author_row.try_get("series_id")?;
                let author = AuthorShort {
                    id: author_row.try_get("id")?,
                    first_name: author_row.try_get("first_name")?,
                    last_name: author_row.try_get("last_name")?,
                };
                series_authors_map
                    .entry(series_id)
                    .or_insert_with(Vec::new)
                    .push(author);
            }
        }

        for item in res.iter_mut() {
            if let Some(ebook_id) = item.ebook_id {
                item.authors = authors_map.remove(&ebook_id);
            }
            if let Some(series_id) = item.series_id {
                item.authors = series_authors_map.remove(&series_id);
            }
        }

        let batch = Batch {
            rows: res,
            total: count,
            offset: params.offset,
            limit: params.limit,
        };
        Ok(batch)
    }

    // Get bookshelf item

    // Add item to bookshelf - can only add to owned bookshelf

    // Remove item from bookshelf - can only remove from owned bookshelf

    // Update item in bookshelf - can only update owned bookshelf
}
