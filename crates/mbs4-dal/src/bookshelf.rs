use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};
use sqlx::{
    Acquire, Database, Executor, FromRow, Row,
    query::{QueryAs, QueryScalar},
};

use crate::{
    Batch, ChosenDB, ChosenRow, Error,
    author::AuthorShort,
    author_utils::{AuthorsQueryBuilder, SeriesAuthorsQueryBuilder, query_authors},
    error::Result,
    now,
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

#[derive(Debug, Deserialize, Clone, garde::Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateBookshelfItem {
    #[garde(length(min = 1, max = 255))]
    pub note: Option<String>,
    #[garde(pattern("^(EBOOK|SERIES)$"))]
    pub item_type: String,
    #[garde(range(min = 0))]
    pub ebook_id: Option<i64>,
    #[garde(range(min = 0))]
    pub series_id: Option<i64>,
    #[garde(skip)]
    pub order: Option<i64>,
    #[garde(length(min = 1, max = 255))]
    pub created_by: Option<String>,
}

#[derive(Debug, Deserialize, Clone, garde::Validate, sqlx::FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateBookshelfItem {
    #[garde(range(min = 0))]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    pub note: Option<String>,
    #[garde(skip)]
    pub order: Option<i64>,
    #[garde(range(min = 0))]
    pub version: i64,
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
        let title: Option<String> = row.try_get("title")?;
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
            title: title.unwrap_or_default(),
            has_cover,
            authors,
            series_title: ebook_series_title,
            series_index: ebook_series_index,
        })
    }
}

const ITEM_VALID_ORDER_FIELDS: &[&str] = &["created", "modified", "order", "id", "title"];

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
            .push(" WHERE public = true AND ( created_by IS NULL OR created_by !=  ");
        self.builder.push_bind(user.into());
        self.builder.push(" ) ");
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
    CASE
        WHEN i.type = 'EBOOK' THEN e.title
        WHEN i.type = 'SERIES' THEN s.title
        ELSE NULL
    END AS title,
    e.cover as ebook_cover,
    e.series_index as ebook_series_index,
    ebs.title as ebook_series_title
from bookshelf_item i
    LEFT OUTER JOIN ebook e ON i.ebook_id = e.id
    LEFT OUTER JOIN series s ON i.series_id = s.id
    LEFT OUTER JOIN series ebs ON e.series_id = ebs.id
where i.bookshelf_id = ?
{order}
limit ? offset ?;"
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

        let authors_query = AuthorsQueryBuilder::new();
        let mut authors_map = query_authors(
            authors_query,
            res.iter().filter_map(|e| e.ebook_id),
            &self.executor,
            "ebook_id",
        )
        .await?;

        let authors_query = SeriesAuthorsQueryBuilder::new();
        let mut series_authors_map = query_authors(
            authors_query,
            res.iter().filter_map(|e| e.series_id),
            &self.executor,
            "series_id",
        )
        .await?;

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

    // Add item to bookshelf
    pub async fn add_item(&self, bookshelf_id: i64, item: CreateBookshelfItem) -> Result<i64> {
        let mut tx = self.executor.begin().await?;
        let existing_id: Option<UpdateBookshelfItem> = sqlx::query_as("SELECT id, note, \"order\", version FROM bookshelf_item WHERE bookshelf_id = ? AND type = ? AND ( ebook_id IS NOT NULL AND ebook_id = ? OR series_id IS NOT NULL AND series_id = ?)")
            .bind(bookshelf_id)
            .bind(&item.item_type)
            .bind(item.ebook_id)
            .bind(item.series_id)
            .fetch_optional(&mut *tx)
            .await?;
        let now = now();
        if let Some(existing) = existing_id {
            if existing.note != item.note || existing.order != item.order {
                sqlx::query("UPDATE bookshelf_item SET note = ?, \"order\" = ?, modified = ?, version = version + 1 WHERE id = ? and version = ?")
                    .bind(item.note)
                    .bind(item.order)
                    .bind(now)
                    .bind(existing.id)
                    .bind(existing.version)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            return Ok(existing.id);
        }

        let id = sqlx::query(r#"INSERT INTO bookshelf_item (version, bookshelf_id, type, ebook_id, series_id, note, "order", created_by, created, modified) VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#)
            .bind(bookshelf_id)
            .bind(item.item_type)
            .bind(item.ebook_id)
            .bind(item.series_id)
            .bind(item.note)
            .bind(item.order)
            .bind(item.created_by)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
        tx.commit().await?;
        Ok(id)
    }

    // Remove item from bookshelf
    pub async fn remove_item(&self, bookshelf_id: i64, id: i64) -> Result<()> {
        let removed = sqlx::query("DELETE FROM bookshelf_item WHERE bookshelf_id = ? AND id = ?")
            .bind(bookshelf_id)
            .bind(id)
            .execute(&self.executor)
            .await?
            .rows_affected();

        if removed == 0 {
            return Err(Error::RecordNotFound("Bookshelf item".to_string()));
        }
        Ok(())
    }

    // Update item in bookshelf
    pub async fn update_item(&self, bookshelf_id: i64, item: UpdateBookshelfItem) -> Result<i64> {
        let updated = sqlx::query("UPDATE bookshelf_item SET note = ?, \"order\" = ?, modified = ?, version = version + 1 WHERE bookshelf_id = ? AND id = ? and version = ?")
            .bind(item.note)
            .bind(item.order)
            .bind(now())
            .bind(bookshelf_id)
            .bind(item.id)
            .bind(item.version)
            .execute(&self.executor)
            .await?
            .rows_affected();

        if updated == 0 {
            return Err(Error::RecordNotFound("Bookshelf item".to_string()));
        }
        Ok(item.id)
    }
}
