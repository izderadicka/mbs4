use crate::{
    dal::user::{CreateUser, UserRepository},
    error::ApiResult,
};

use axum::{
    extract::Path,
    response::IntoResponse,
    routing::{delete, post},
    Json,
};
use http::StatusCode;

use crate::state::AppState;

pub async fn create_user(
    user_registry: UserRepository,
    Json(payload): Json<CreateUser>,
) -> ApiResult<impl IntoResponse> {
    let user = user_registry.create(payload).await?;

    Ok((StatusCode::CREATED, Json(user)))
}

async fn list_users(user_registry: UserRepository) -> ApiResult<impl IntoResponse> {
    let users = user_registry.list(100).await?;
    Ok((StatusCode::OK, Json(users)))
}

async fn delete_user(
    Path(id): Path<i64>,
    user_registry: UserRepository,
) -> ApiResult<impl IntoResponse> {
    user_registry.delete(id).await?;

    Ok((StatusCode::NO_CONTENT, ()))
}

pub fn users_router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", post(create_user).get(list_users))
        .route("/{id}", delete(delete_user))
}
