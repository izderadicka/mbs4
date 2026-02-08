use crate::{auth::token::RequiredRolesLayer, crud_api, publish_api_docs};
use garde::Validate;
#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::author::{Author, AuthorRepository, AuthorShort};
use mbs4_types::claim::Role;

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};

publish_api_docs!(
    extra_crud_api::list_ebooks,
    extra_crud_api::create,
    extra_crud_api::update,
    extra_crud_api::delete,
    extra_crud_api::merge
);
crud_api!(Author, RO);

#[derive(serde::Deserialize, Debug, Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AuthorMergeRequest {
    #[garde(range(min = 0))]
    author_id: i64,
}

mod extra_crud_api {
    use axum::{
        extract::{Path, Query, State},
        response::IntoResponse,
        Json,
    };
    use axum_valid::Garde;
    use http::StatusCode;
    use mbs4_dal::author::{AuthorRepository, AuthorShort, CreateAuthor, UpdateAuthor};
    #[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
    use mbs4_dal::ebook::{EbookRepository, EbookShort};
    use mbs4_types::claim::ApiClaim;

    use crate::{
        error::ApiResult,
        rest_api::{
            author::AuthorMergeRequest,
            indexing::{reindex_books, DependentId},
            Paging,
        },
        state::AppState,
    };

    #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/{id}/ebooks", tag = "Author", operation_id = "listAuthorEbook",
        params(Paging), responses((status = StatusCode::OK, description = "List of Author Ebooks paginated", body = crate::rest_api::Page<EbookShort>))))]
    pub async fn list_ebooks(
        Path(author_id): Path<i64>,
        repository: EbookRepository,
        State(state): State<AppState>,
        Garde(Query(paging)): Garde<Query<Paging>>,
    ) -> ApiResult<impl IntoResponse> {
        let default_page_size: u32 = state.config().default_page_size;
        let page_size = paging.page_size(default_page_size);
        let listing_params = paging.into_listing_params(default_page_size)?;
        let batch = repository.list_by_author(listing_params, author_id).await?;
        Ok((
            StatusCode::OK,
            Json(crate::rest_api::Page::from_batch(batch, page_size)),
        ))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "", tag = "Author", operation_id = "createAuthor",
            responses((status = StatusCode::CREATED, description = "Created Author", body = mbs4_dal::author::Author))))]
    pub async fn create(
        repository: AuthorRepository,
        State(state): State<AppState>,
        api_user: ApiClaim,
        Garde(Json(mut payload)): Garde<Json<CreateAuthor>>,
    ) -> ApiResult<impl IntoResponse> {
        payload.created_by = Some(api_user.sub);
        let record = repository.create(payload).await?;

        if let Err(e) = state.search().index_author(
            AuthorShort {
                id: record.id,
                first_name: record.first_name.clone(),
                last_name: record.last_name.clone(),
            },
            false,
        ) {
            tracing::error!("Failed to index author: {}", e);
        }

        Ok((StatusCode::CREATED, Json(record)))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(put, path = "/{id}", tag = "Author", operation_id = "updateAuthor",
            responses((status = StatusCode::OK, description = "Updated Author", body = mbs4_dal::author::Author))))]
    pub async fn update(
        Path(id): Path<i64>,
        repository: AuthorRepository,
        ebook_repo: EbookRepository,
        State(state): State<AppState>,
        Garde(Json(payload)): Garde<Json<UpdateAuthor>>,
    ) -> ApiResult<impl IntoResponse> {
        let record = repository.update(id, payload).await?;

        if let Err(e) = state.search().index_author(
            AuthorShort {
                id: record.id,
                first_name: record.first_name.clone(),
                last_name: record.last_name.clone(),
            },
            true,
        ) {
            tracing::error!("Failed to index author: {}", e);
        }

        if let Err(e) = reindex_books(&ebook_repo, state.search(), DependentId::Author(id)).await {
            tracing::error!("Error reindexing ebooks based on author update: {e}");
        }

        Ok((StatusCode::OK, Json(record)))
    }

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(delete, path = "/{id}", tag = "Author", operation_id = "deleteAuthor")
    )]
    pub async fn delete(
        Path(id): Path<i64>,
        repository: AuthorRepository,
        ebook_repo: EbookRepository,
        State(state): State<AppState>,
    ) -> ApiResult<impl IntoResponse> {
        repository.delete(id).await?;

        if let Err(e) = state.search().delete_author(id) {
            tracing::error!("Failed to delete in author index: {}", e);
        }

        if let Err(e) = reindex_books(&ebook_repo, state.search(), DependentId::Author(id)).await {
            tracing::error!("Error reindexing ebooks based on author update: {e}");
        }

        Ok((StatusCode::NO_CONTENT, ()))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(put, path = "/{id}/merge", tag = "Author", operation_id = "mergeAuthor",
        request_body = AuthorMergeRequest,
        responses((status = StatusCode::OK, description = "Merge author to other author"))))]
    pub async fn merge(
        Path(id): Path<i64>,
        repository: AuthorRepository,
        ebook_repo: EbookRepository,
        State(state): State<AppState>,
        Garde(Json(merge_request)): Garde<Json<AuthorMergeRequest>>,
    ) -> ApiResult<impl IntoResponse> {
        let from_id = merge_request.author_id;
        repository.merge(from_id, id).await?;

        if let Err(e) = state.search().delete_author(from_id) {
            tracing::error!("Failed to delete in author index: {}", e);
        }
        if let Err(e) = reindex_books(&ebook_repo, state.search(), DependentId::Author(id)).await {
            tracing::error!("Error reindexing ebooks based on author update: {e}");
        }
        Ok((StatusCode::OK, ()))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{id}", delete(extra_crud_api::delete))
        .route("/{id}/merge", put(extra_crud_api::merge))
        .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/", post(extra_crud_api::create))
        .route("/{id}", put(extra_crud_api::update))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
        .route("/{id}/ebooks", get(extra_crud_api::list_ebooks))
}
