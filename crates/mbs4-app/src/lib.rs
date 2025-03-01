pub mod auth;
pub mod dal;
pub mod error;
pub mod state;
pub mod user;

pub type ChosenDB = sqlx::Sqlite;
