use crate::error::{ApiResult, Result};
use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, post},
    Json,
};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::state::AppState;

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string();
    Ok(password_hash)
}

#[allow(dead_code)]
fn verify_password(password: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(&password)?;
    let res = Argon2::default().verify_password(password.as_bytes(), &parsed_hash);
    if let Err(e) = res {
        debug!("Invalid password, error {e}");
    }
    Ok(res.is_ok())
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub roles: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateUser {
    pub email: String,
    pub name: Option<String>,
    pub password: Option<String>,
    pub roles: Option<Vec<String>>,
}

pub async fn create_user(
    State(state): State<AppState>,
    Json(payload): Json<CreateUser>,
) -> ApiResult<impl IntoResponse> {
    let password = payload.password.map(|p| hash_password(&p)).transpose()?;
    let roles = payload.roles.map(|roles| roles.join(","));
    let result =
        sqlx::query("INSERT INTO users (name, email, password, roles) VALUES (?, ?, ?, ?)")
            .bind(&payload.name)
            .bind(&payload.email)
            .bind(password)
            .bind(roles)
            .execute(state.pool())
            .await?;

    let id = result.last_insert_rowid();
    let user: User = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(state.pool())
        .await?;

    Ok((StatusCode::CREATED, Json(user)))
}

async fn list_users(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let users = sqlx::query_as::<_, User>("SELECT id, name, email, roles FROM users")
        .fetch_all(state.pool())
        .await?;
    Ok((StatusCode::OK, Json(users)))
}

async fn delete_user(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> ApiResult<impl IntoResponse> {
    // First check if the user exists
    match sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(state.pool())
        .await?
    {
        Some(_id) => {
            // User exists, proceed with deletion
            sqlx::query("DELETE FROM users WHERE id = ?")
                .bind(id)
                .execute(state.pool())
                .await?;

            Ok(StatusCode::NO_CONTENT)
        }
        None => Err(crate::error::ApiError::ResourceNotFound("User".to_string())),
    }
}

pub fn users_router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", post(create_user).get(list_users))
        .route("/{id}", delete(delete_user))
}
