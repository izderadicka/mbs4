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
    use mbs4_dal::author::UpdateAuthor;
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

    /*************  ✨ Windsurf Command ⭐  *************/
    /// Update an author with the given ID.
    ///
    /// The payload should contain the updated fields.
    /*******  7b5b73bf-6884-4b6f-896e-27fdb55681e0  *******/
    pub async fn update(
        Path(id): Path<i64>,
        repository: AuthorRepository,
        Garde(Json(payload)): Garde<Json<UpdateAuthor>>,
    ) -> ApiResult<impl IntoResponse> {
        let record = repository.update(id, payload).await?;

        Ok((StatusCode::OK, Json(record)))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", post(crud_api::create))
        .route("/{id}", put(crud_api::update))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        // .route("/", get(crud_api::list))
        // .route("/count", get(crud_api::count))
        // .route("/all", get(crud_api::list_all))
        .route("/{id}", get(crud_api::get))
}
