use crate::{auth::token::RequiredRolesLayer, crud_api};
use mbs4_dal::author::{AuthorRepository, CreateAuthor};
use mbs4_types::claim::Role;

use crate::repository_from_request;
use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};

repository_from_request!(AuthorRepository);

pub mod crud_api {
    use super::*;
    use crate::error::ApiResult;
    use axum::{extract::Path, response::IntoResponse, Json};
    use axum_valid::Garde;
    use http::StatusCode;
    pub async fn create(
        repository: AuthorRepository,
        Garde(Json(payload)): Garde<Json<CreateAuthor>>,
    ) -> ApiResult<impl IntoResponse> {
        let record = repository.create(payload).await?;

        Ok((StatusCode::CREATED, Json(record)))
    }

    pub async fn get(
        Path(id): Path<i64>,
        repository: AuthorRepository,
    ) -> ApiResult<impl IntoResponse> {
        let record = repository.get(id).await?;

        Ok((StatusCode::OK, Json(record)))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", post(crud_api::create))
        // .route("/{id}", delete(crud_api::delete).put(crud_api::update))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        // .route("/", get(crud_api::list))
        // .route("/count", get(crud_api::count))
        // .route("/all", get(crud_api::list_all))
        .route("/{id}", get(crud_api::get))
}
