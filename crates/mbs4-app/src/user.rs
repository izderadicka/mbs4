use crate::{auth::token::RequiredRolesLayer, error::ApiResult, repository_from_request};
use axum_valid::Garde;
use mbs4_dal::user::{CreateUser, User, UserRepository};

use axum::{
    extract::Path,
    response::IntoResponse,
    routing::{delete, post},
    Json,
};
use http::StatusCode;
use mbs4_types::claim::Role;

use crate::state::AppState;

repository_from_request!(UserRepository);

#[cfg(feature = "openapi")]
#[derive(utoipa::OpenApi)]
#[openapi(paths(create_user, list_users, delete_user))]
struct ModuleDocs;

#[cfg(feature = "openapi")]
pub fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    ModuleDocs::openapi()
}

#[cfg_attr(feature = "openapi",  utoipa::path(post, path = "", tag = "Users", operation_id = "createUser",
    responses((status = StatusCode::CREATED, description = "Create new User", body = User))))]
pub async fn create_user(
    user_registry: UserRepository,
    Garde(Json(payload)): Garde<Json<CreateUser>>,
) -> ApiResult<impl IntoResponse> {
    let user = user_registry.create(payload).await?;

    Ok((StatusCode::CREATED, Json(user)))
}

#[cfg_attr(feature = "openapi",  utoipa::path(get, path = "", tag = "Users", operation_id = "listUsers",
    responses((status = StatusCode::OK, description = "List Users", body = Vec<User>))))]
async fn list_users(user_registry: UserRepository) -> ApiResult<impl IntoResponse> {
    let users = user_registry.list(100).await?;
    Ok((StatusCode::OK, Json(users)))
}

#[cfg_attr(feature = "openapi",  utoipa::path(delete, path = "/{id}", tag = "Users", operation_id = "deleteUser",
    responses((status = StatusCode::NO_CONTENT, description = "Deleted successfully"))))]
async fn delete_user(
    Path(id): Path<i64>,
    user_registry: UserRepository,
) -> ApiResult<impl IntoResponse> {
    user_registry.delete(id).await?;

    Ok((StatusCode::NO_CONTENT, ()))
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", post(create_user).get(list_users))
        .route("/{id}", delete(delete_user))
        .layer(RequiredRolesLayer::new([Role::Admin]))
}
