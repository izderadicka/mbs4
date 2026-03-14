use std::collections::HashMap;

use futures::TryStreamExt;
use sqlx::{Database, Executor, Row, query::Query};

use crate::{ChosenDB, author::AuthorShort, error::Result};

pub trait AuthorsQuery {
    fn limit_for<I>(&mut self, ids: I)
    where
        I: IntoIterator<Item = i64>;

    fn build_query<'q>(
        &'q mut self,
    ) -> Query<'q, ChosenDB, <ChosenDB as Database>::Arguments<'static>>;

    fn is_empty(&self) -> bool;
}

pub struct SeriesAuthorsQueryBuilder {
    builder: sqlx::query_builder::QueryBuilder<'static, ChosenDB>,
    has_limit: bool,
}

impl SeriesAuthorsQueryBuilder {
    pub fn new() -> Self {
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
}

impl AuthorsQuery for SeriesAuthorsQueryBuilder {
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

pub struct AuthorsQueryBuilder {
    builder: sqlx::query_builder::QueryBuilder<'static, ChosenDB>,
    has_limit: bool,
}

impl AuthorsQueryBuilder {
    pub fn new() -> Self {
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

    fn _finish(&mut self) {
        self.builder.push(
            "
    ) t
WHERE rn <= 3;",
        );
    }
}

impl AuthorsQuery for AuthorsQueryBuilder {
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

pub async fn query_authors<'c, AQ, IDS, EX>(
    mut authors_query: AQ,
    ids: IDS,
    executor: EX,
    id_key: &str,
) -> Result<HashMap<i64, Vec<AuthorShort>>>
where
    AQ: AuthorsQuery + Send,
    IDS: IntoIterator<Item = i64>,
    EX: Executor<'c, Database = ChosenDB>,
{
    authors_query.limit_for(ids);
    let mut authors_map = HashMap::new();
    if !authors_query.is_empty() {
        let mut authors = authors_query.build_query().fetch(executor);
        while let Some(author_row) = authors.try_next().await? {
            let id: i64 = author_row.try_get(id_key)?;
            let author = AuthorShort {
                id: author_row.try_get("id")?,
                first_name: author_row.try_get("first_name")?,
                last_name: author_row.try_get("last_name")?,
            };
            authors_map.entry(id).or_insert_with(Vec::new).push(author);
        }
    }
    Ok(authors_map)
}
