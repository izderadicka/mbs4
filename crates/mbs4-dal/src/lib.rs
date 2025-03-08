pub mod error;
pub mod user;

pub use error::Error;
pub use sqlx::Error as SqlxError;
use sqlx::sqlite::SqlitePoolOptions;

pub type ChosenDB = sqlx::Sqlite;
pub type Pool = sqlx::Pool<ChosenDB>;

pub async fn new_pool(database_url: &str) -> Result<Pool, Error> {
    let pool = SqlitePoolOptions::new()
        .max_connections(50)
        .connect(database_url)
        .await?;
    Ok(pool)
}
