use crate::{Error, ListingParams, MAX_LIMIT, error::Result};
use futures::{StreamExt as _, TryStreamExt as _};
use garde::Validate;
use serde::{Deserialize, Serialize};
use sqlx::{Acquire, Executor, Pool};
use tracing::debug;

#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub struct CreateLanguage {
    #[garde(length(min = 1, max = 255))]
    name: String,
    #[garde(length(min = 2, max = 4))]
    code: String,
    #[garde(range(min = 0))]
    version: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Language {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub version: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct LanguageShort {
    pub id: i64,
    pub name: String,
    pub code: String,
}

pub type LanguageRepository = LanguageRepositoryImpl<Pool<crate::ChosenDB>>;

pub struct LanguageRepositoryImpl<E> {
    executor: E,
}

impl<'c, E> LanguageRepositoryImpl<E>
where
    for<'a> &'a E:
        Executor<'c, Database = crate::ChosenDB> + Acquire<'c, Database = crate::ChosenDB>,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub async fn create(&self, payload: CreateLanguage) -> Result<Language> {
        let result = sqlx::query("INSERT INTO language (name, code, version) VALUES (?, ?, 1)")
            .bind(&payload.name)
            .bind(&payload.code)
            .execute(&self.executor)
            .await?;

        let id = result.last_insert_rowid();
        self.get(id).await
    }

    pub async fn update(&self, id: i64, payload: CreateLanguage) -> Result<Language> {
        let version = payload.version.ok_or_else(|| {
            debug!("No version provided");
            Error::MissingVersion
        })?;
        let mut conn = self.executor.acquire().await?;
        let mut transaction = conn.begin().await?;
        let result = sqlx::query(
            "UPDATE language SET name = ?, code = ?, version = ? WHERE id = ? and version = ?",
        )
        .bind(&payload.name)
        .bind(&payload.code)
        .bind(version + 1)
        .bind(id)
        .bind(version)
        .execute(&mut *transaction)
        .await?;

        if result.rows_affected() == 0 {
            Err(Error::FailedUpdate { id, version })
        } else {
            let record = get(id, &mut *transaction).await?;
            transaction.commit().await?;
            Ok(record)
        }
    }

    pub async fn count(&self) -> Result<u64> {
        let count: u64 = sqlx::query_scalar("SELECT count(*) FROM language")
            .fetch_one(&self.executor)
            .await?;
        Ok(count)
    }

    pub async fn list_all(&self) -> Result<Vec<LanguageShort>> {
        self.list(ListingParams::default()).await
    }

    pub async fn list(&self, params: ListingParams) -> Result<Vec<LanguageShort>> {
        let order = ""; // This is safety limit
        let records = sqlx::query_as::<_, LanguageShort>(&format!(
            "SELECT id, name, code FROM language {order} LIMIT ? OFFSET ?"
        ))
        .bind(params.limit)
        .bind(params.offset)
        .fetch(&self.executor)
        .take(MAX_LIMIT)
        .try_collect::<Vec<_>>()
        .await?;
        Ok(records)
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let res = sqlx::query("DELETE FROM language WHERE id = ?")
            .bind(id)
            .execute(&self.executor)
            .await?;

        if res.rows_affected() == 0 {
            Err(crate::error::Error::RecordNotFound("Language".to_string()))
        } else {
            Ok(())
        }
    }

    pub async fn get(&self, id: i64) -> Result<Language> {
        get(id, &self.executor).await
    }
}

async fn get<'c, E>(id: i64, executor: E) -> Result<Language>
where
    E: Executor<'c, Database = crate::ChosenDB>,
{
    let record: Language = sqlx::query_as!(Language, "SELECT * FROM language WHERE id = ?", id)
        .fetch_one(executor)
        .await?
        .into();
    Ok(record)
}
