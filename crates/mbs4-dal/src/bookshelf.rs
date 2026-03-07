use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};
use sqlx::{
    Acquire, Database, Executor, FromRow,
    query::{QueryAs, QueryScalar},
};

use crate::{Batch, ChosenDB};

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

// const ITEM_VALID_ORDER_FIELDS: &[&str] = &["created", "modified", "order", "id"];

struct QueryBuilder {
    builder: sqlx::query_builder::QueryBuilder<'static, ChosenDB>,
    is_count: bool,
}

impl QueryBuilder {
    fn new() -> Self {
        let builder = sqlx::query_builder::QueryBuilder::new(
            "SELECT *, (select count(*) from bookshelf_item where bookshelf_id = bookshelf.id) as items_count FROM bookshelf ",
        );
        QueryBuilder {
            builder,
            is_count: false,
        }
    }

    fn new_count_query() -> Self {
        let builder = sqlx::query_builder::QueryBuilder::new("SELECT COUNT(*) FROM bookshelf ");
        QueryBuilder {
            builder,
            is_count: true,
        }
    }

    fn private(&mut self, user: impl Into<String>) -> &mut Self {
        self.builder.push(" WHERE created_by = ? ");
        self.builder.push_bind(user.into());
        self
    }

    fn public(&mut self, user: impl Into<String>) -> &mut Self {
        self.builder
            .push(" WHERE public = true AND created_by != ? ");
        self.builder.push_bind(user.into());
        self
    }

    fn order_and_limit(
        &mut self,
        params: &crate::ListingParams,
    ) -> crate::error::Result<&mut Self> {
        let order = params.ordering(VALID_ORDER_FIELDS)?;
        if !order.is_empty() {
            self.builder.push(format!(" {order} "));
        }
        self.builder.push(" LIMIT ? OFFSET ?");
        self.builder.push_bind(params.limit);
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
    ) -> crate::error::Result<Batch<BookshelfListing>> {
        let mut builder = QueryBuilder::new();
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
        let mut builder = QueryBuilder::new_count_query();
        if private {
            builder.private(user);
        } else {
            builder.public(user);
        }

        let count_query = builder.private(user).build_count_query();
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
    ) -> crate::error::Result<Batch<BookshelfListing>> {
        self.list_owned(user, true, params).await
    }

    // List others public bookshelves - created by other users and

    pub async fn list_public(
        &self,
        user: &str,
        params: crate::ListingParams,
    ) -> crate::error::Result<Batch<BookshelfListing>> {
        self.list_owned(user, false, params).await
    }

    // List bookshelf items, paged, sorted - can list only public or mine

    // Add item to bookshelf - can only add to owned bookshelf

    // Remove item from bookshelf - can only remove from owned bookshelf

    // Update item in bookshelf - can only update owned bookshelf
}
