use std::future::Future;

use argon2::{
    password_hash::{rand_core::OsRng, Result as HashResult, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use axum::extract::FromRequestParts;
use futures::StreamExt;
use http::{request::Parts, StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::Pool;
use tracing::debug;

use crate::{error::ApiResult, state::AppState};

fn hash_password(password: &str) -> HashResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string();
    Ok(password_hash)
}

#[allow(dead_code)]
fn verify_password(password: &str) -> HashResult<bool> {
    let parsed_hash = PasswordHash::new(password)?;
    let res = Argon2::default().verify_password(password.as_bytes(), &parsed_hash);
    if let Err(e) = res {
        debug!("Invalid password, error {e}");
    }
    Ok(res.is_ok())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateUser {
    pub email: String,
    pub name: Option<String>,
    pub password: Option<String>,
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

    pub async fn create(&self, payload: CreateUser) -> ApiResult<User> {
        let password = payload.password.map(|p| hash_password(&p)).transpose()?;
        let roles = payload.roles.map(|roles| roles.join(","));
        let result = sqlx::query!(
            "INSERT INTO users (name, email, password, roles) VALUES (?, ?, ?, ?)",
            payload.name,
            payload.email,
            password,
            roles
        )
        .execute(&self.executor)
        .await?;

        let id = result.last_insert_rowid();
        let user: User = sqlx::query_as::<_, UserInt>("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_one(&self.executor)
            .await?
            .into();

        Ok(user)
    }

    pub async fn list(&self, limit: usize) -> ApiResult<Vec<User>> {
        let users = sqlx::query_as::<_, UserInt>("SELECT id, name, email, roles FROM users")
            .fetch(&self.executor)
            .take(limit)
            .filter_map(|r| async move { r.ok().map(User::from) })
            .collect::<Vec<_>>()
            .await;
        Ok(users)
    }

    pub async fn delete(&self, id: i64) -> ApiResult<()> {
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
            None => Err(crate::error::ApiError::ResourceNotFound("User".to_string())),
        }
    }

    pub async fn get(&self, id: i64) -> ApiResult<User> {
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

    pub async fn find_by_email(&self, email: &str) -> ApiResult<User> {
        let user: User = sqlx::query_as::<_, UserInt>("SELECT * FROM users WHERE email = ?")
            .bind(email)
            .fetch_one(&self.executor)
            .await?
            .into();
        Ok(user)
    }
}

impl FromRequestParts<AppState> for UserRepositoryImpl<Pool<crate::ChosenDB>> {
    type Rejection = StatusCode;

    fn from_request_parts(
        _parts: &mut Parts,
        state: &AppState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        futures::future::ready(Ok(UserRepositoryImpl::new(state.pool().clone())))
    }
}
