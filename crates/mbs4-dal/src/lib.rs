pub mod author;
pub mod ebook;
pub mod error;
pub mod format;
pub mod genre;
pub mod language;
pub mod series;
pub mod source;
pub mod user;

use std::fmt::Display;

pub use error::Error;
pub use sqlx::Error as SqlxError;
use sqlx::sqlite::SqlitePoolOptions;

use crate::error::Result;

pub type ChosenDB = sqlx::Sqlite;
pub type ChosenRow = <crate::ChosenDB as sqlx::Database>::Row;
pub type Pool = sqlx::Pool<ChosenDB>;

pub const MAX_LIMIT: usize = 10_000;
pub const DEFAULT_PAGE_SIZE: i64 = 100;

pub async fn new_pool(database_url: &str) -> Result<Pool, Error> {
    let pool = SqlitePoolOptions::new()
        .max_connections(50)
        .connect(database_url)
        .await?;
    Ok(pool)
}

#[derive(Debug, Clone)]
pub enum Filter {
    Genres(Vec<i64>),
}

#[derive(Debug, Clone)]
pub enum Order {
    Asc(String),
    Desc(String),
}

impl Display for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Order::Asc(s) => write!(f, "{}", s),
            Order::Desc(s) => write!(f, "{} DESC", s),
        }
    }
}

impl AsRef<str> for Order {
    fn as_ref(&self) -> &str {
        match self {
            Order::Asc(s) => s.as_str(),
            Order::Desc(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ListingParams {
    pub offset: i64,
    pub limit: i64,
    pub order: Option<Vec<Order>>,
    pub filter: Option<Vec<Filter>>,
}

impl Default for ListingParams {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_PAGE_SIZE,
            order: None,
            filter: None,
        }
    }
}

impl ListingParams {
    pub fn new(offset: i64, limit: i64) -> Self {
        Self {
            offset,
            limit,
            order: None,
            filter: None,
        }
    }

    pub fn new_unpaged() -> Self {
        Self {
            offset: 0,
            limit: MAX_LIMIT as i64,
            order: None,
            filter: None,
        }
    }
    pub fn with_order(mut self, order: Vec<Order>) -> Self {
        self.order = Some(order);
        self
    }

    pub fn ordering(&self, valid_fields: &[&str]) -> Result<String> {
        let ordering = self
            .order
            .as_ref()
            .map(|o| {
                o.iter()
                    .map(|o| {
                        if valid_fields.contains(&o.as_ref()) {
                            Ok(o.to_string())
                        } else {
                            Err(Error::InvalidOrderByField(o.as_ref().to_string()))
                        }
                    })
                    .collect::<Result<Vec<String>>>()
                    .map(|o| o.join(", "))
            })
            .transpose()?
            .map(|o| {
                if o.is_empty() {
                    o
                } else {
                    format!("ORDER BY {}", o)
                }
            })
            .unwrap_or_default();
        Ok(ordering)
    }

    pub fn genres_filter(&self) -> Option<&Vec<i64>> {
        self.filter.as_ref().and_then(|f| {
            f.iter().find_map(|f| match f {
                Filter::Genres(genres) => Some(genres),
                #[allow(unreachable_patterns)]
                _ => None,
            })
        })
    }
}

pub struct Batch<T> {
    pub offset: i64,
    pub limit: i64,
    pub total: u64,
    pub rows: Vec<T>,
}

pub trait FromRowPrefixed<'r, R>: Sized
where
    R: sqlx::Row,
{
    fn from_row_prefixed(row: &'r R) -> Result<Self, sqlx::Error>;
}

pub(crate) fn now() -> time::PrimitiveDateTime {
    let now = time::OffsetDateTime::now_utc();
    time::PrimitiveDateTime::new(now.date(), now.time())
}
