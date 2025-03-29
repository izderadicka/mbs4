pub mod error;
pub mod language;
pub mod user;

pub use error::Error;
pub use sqlx::Error as SqlxError;
use sqlx::sqlite::SqlitePoolOptions;

pub type ChosenDB = sqlx::Sqlite;
pub type Pool = sqlx::Pool<ChosenDB>;

const MAX_LIMIT: usize = 10_000;

pub async fn new_pool(database_url: &str) -> Result<Pool, Error> {
    let pool = SqlitePoolOptions::new()
        .max_connections(50)
        .connect(database_url)
        .await?;
    Ok(pool)
}

#[derive(Debug, Clone)]
pub enum Order {
    Asc(String),
    Desc(String),
}
pub struct ListingParams {
    offset: i64,
    limit: i64,
    order: Option<Vec<Order>>,
}

impl Default for ListingParams {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: MAX_LIMIT as i64,
            order: None,
        }
    }
}
