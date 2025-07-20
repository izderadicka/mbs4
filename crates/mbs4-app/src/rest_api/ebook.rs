use mbs4_dal::ebook::{Ebook, EbookRepository, EbookShort};
use mbs4_types::claim::Role;
use utoipa::{openapi::OpenApi, OpenApi as _};

use crate::{auth::token::RequiredRolesLayer, crud_api, state::AppState};
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};
// crate::repository_from_request!(EbookRepository);

#[derive(utoipa::OpenApi)]
#[openapi(paths(crud_api_write::create, crud_api_write::update, crud_api_write::delete))]
struct ModuleDocs;

pub fn api_docs() -> OpenApi {
    let docs = ModuleDocs::openapi();
    docs.merge_from(crud_api::api_docs())
}

crud_api!(Ebook, RO);

mod crud_api_write {
    use axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    };
    use axum_valid::Garde;
    use http::StatusCode;
    use mbs4_dal::ebook::{CreateEbook, Ebook, EbookRepository, UpdateEbook};

    use crate::{error::ApiResult, state::AppState};

    #[utoipa::path(post, path = "/", responses((status = StatusCode::CREATED, description = "Created", body = Ebook)))]
    pub async fn create(
        repository: EbookRepository,
        State(state): State<AppState>,
        Garde(Json(payload)): Garde<Json<CreateEbook>>,
    ) -> ApiResult<impl IntoResponse> {
        let record = repository.create(payload).await?;
        if let Err(e) = state.search().index_book(record.clone(), false) {
            tracing::error!("Failed to index book: {}", e);
        }

        Ok((StatusCode::CREATED, Json(record)))
    }

    #[utoipa::path(put, path = "/{id}", responses((status = StatusCode::OK, description = "Updated", body = Ebook)))]
    pub async fn update(
        Path(id): Path<i64>,
        repository: EbookRepository,
        State(state): State<AppState>,
        Garde(Json(payload)): Garde<Json<UpdateEbook>>,
    ) -> ApiResult<impl IntoResponse> {
        let record = repository.update(id, payload).await?;
        if let Err(e) = state.search().index_book(record.clone(), true) {
            tracing::error!("Failed to index book: {}", e);
        }

        Ok((StatusCode::OK, Json(record)))
    }

    #[utoipa::path(delete, path = "/{id}")]
    pub async fn delete(
        Path(id): Path<i64>,
        repository: EbookRepository,
        State(state): State<AppState>,
    ) -> ApiResult<impl IntoResponse> {
        repository.delete(id).await?;

        if let Err(e) = state.search().delete_book(id) {
            tracing::error!("Failed to delete book: {}", e);
        }

        Ok((StatusCode::NO_CONTENT, ()))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{id}", delete(crud_api_write::delete))
        .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/", post(crud_api_write::create))
        .route("/{id}", put(crud_api_write::update))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
}
