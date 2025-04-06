use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{Result as HashResult, SaltString, rand_core::OsRng},
};

use futures::StreamExt as _;
use garde::Validate;
use mbs4_types::{claim::Role, general::ValidEmail};
use serde::{Deserialize, Serialize};
use sqlx::Pool;
use tracing::debug;

use crate::{Error, error::Result};

fn hash_password(password: &str) -> HashResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string();
    Ok(password_hash)
}

#[allow(dead_code)]
fn verify_password(password: &str, password_hash: &str) -> HashResult<bool> {
    let parsed_hash = PasswordHash::new(password_hash)?;
    let res = Argon2::default().verify_password(password.as_bytes(), &parsed_hash);
    if let Err(e) = res {
        debug!("Invalid password, error {e}");
    }
    Ok(res.is_ok())
}

fn is_valid_role(role: &str, _ctx: &()) -> garde::Result {
    role.parse::<Role>()
        .map_err(|e| garde::Error::new(e))
        .map(|_| ())
}

#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub struct CreateUser {
    #[garde(dive)]
    pub email: ValidEmail,
    #[garde(length(min = 3, max = 255))]
    pub name: Option<String>,
    #[garde(length(min = 8, max = 255))]
    pub password: Option<String>,
    #[garde(inner(inner(custom(is_valid_role))))]
    pub roles: Option<Vec<String>>,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct UserInt {
    id: i64,
    name: String,
    email: String,
    roles: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub roles: Option<Vec<String>>,
}

impl From<UserInt> for User {
    fn from(value: UserInt) -> Self {
        Self {
            id: value.id,
            name: value.name,
            email: value.email,
            roles: value.roles.map(|s| {
                s.split(",")
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect()
            }),
        }
    }
}

pub type UserRepository = UserRepositoryImpl<Pool<crate::ChosenDB>>;

pub struct UserRepositoryImpl<E> {
    executor: E,
}

impl<'c, E> UserRepositoryImpl<E>
where
    for<'a> &'a E: sqlx::Executor<'c, Database = crate::ChosenDB>,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub async fn create(&self, payload: CreateUser) -> Result<User> {
        let password = payload.password.map(|p| hash_password(&p)).transpose()?;
        let email = payload.email.as_ref();
        let roles = payload.roles.map(|roles| roles.join(","));
        let result = sqlx::query!(
            "INSERT INTO users (name, email, password, roles) VALUES (?, ?, ?, ?)",
            payload.name,
            email,
            password,
            roles
        )
        .execute(&self.executor)
        .await?;

        let id = result.last_insert_rowid();
        self.get(id).await
    }

    pub async fn list(&self, limit: usize) -> Result<Vec<User>> {
        let users = sqlx::query_as::<_, UserInt>("SELECT id, name, email, roles FROM users")
            .fetch(&self.executor)
            .take(limit)
            .filter_map(|r| async move { r.ok().map(User::from) })
            .collect::<Vec<_>>()
            .await;
        Ok(users)
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        // First check if the user exists
        match sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.executor)
            .await?
        {
            Some(_id) => {
                // User exists, proceed with deletion
                sqlx::query("DELETE FROM users WHERE id = ?")
                    .bind(id)
                    .execute(&self.executor)
                    .await?;

                Ok(())
            }
            None => Err(crate::error::Error::RecordNotFound("User".to_string())),
        }
    }

    pub async fn get(&self, id: i64) -> Result<User> {
        let user: User = sqlx::query_as!(
            UserInt,
            "SELECT id, name, email, roles FROM users WHERE id = ?",
            id
        )
        .fetch_one(&self.executor)
        .await?
        .into();
        Ok(user)
    }

    pub async fn find_by_email(&self, email: &str) -> Result<User> {
        let user: User = sqlx::query_as::<_, UserInt>("SELECT * FROM users WHERE email = ?")
            .bind(email)
            .fetch_one(&self.executor)
            .await?
            .into();
        Ok(user)
    }

    pub async fn check_password(&self, email: &str, password: &str) -> Result<User> {
        let (id, hashed_password): (i64, Option<String>) =
            sqlx::query_as("SELECT id, password FROM users WHERE email = ?")
                .bind(email)
                .fetch_one(&self.executor)
                .await
                .map_err(|e| {
                    debug!("User check error: {e}");
                    Error::InvalidCredentials
                })?;
        if let Some(hashed_password) = hashed_password {
            if verify_password(password, &hashed_password).unwrap_or(false) {
                return self.get(id).await;
            }
        }
        Err(Error::InvalidCredentials)
    }
}
