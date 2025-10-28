use crate::{auth::token::RequiredRolesLayer, crud_api, publish_api_docs};
#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::series::{Series, SeriesRepository, SeriesShort};
use mbs4_types::claim::Role;

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};

publish_api_docs!(
    extra_crud_api::list_ebooks,
    extra_crud_api::create,
    extra_crud_api::update,
    extra_crud_api::delete
);
crud_api!(Series, RO);

mod extra_crud_api {
    use axum::{
        extract::{Path, Query, State},
        response::IntoResponse,
        Json,
    };
    use axum_valid::Garde;
    use http::StatusCode;
    #[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
    use mbs4_dal::ebook::{EbookRepository, EbookShort};
    use mbs4_dal::series::{CreateSeries, Series, SeriesRepository, SeriesShort, UpdateSeries};
    use mbs4_types::claim::ApiClaim;

    use crate::{
        error::ApiResult,
        rest_api::{
            indexing::{reindex_books, DependentId},
            Paging,
        },
        state::AppState,
    };

    #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/{id}/ebooks", tag = "Series", operation_id = "listSeriesEbook",
        params(Paging), responses((status = StatusCode::OK, description = "List of Series Ebooks paginated", body = crate::rest_api::Page<EbookShort>))))]
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

    #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "", tag = "Series", operation_id = "createSeries",
            responses((status = StatusCode::CREATED, description = "Created Series", body = Series))))]
    pub async fn create(
        repository: SeriesRepository,
        State(state): State<AppState>,
        api_user: ApiClaim,
        Garde(Json(mut payload)): Garde<Json<CreateSeries>>,
    ) -> ApiResult<impl IntoResponse> {
        payload.created_by = Some(api_user.sub);
        let record = repository.create(payload).await?;

        if let Err(e) = state.search().index_series(
            SeriesShort {
                id: record.id,
                title: record.title.clone(),
            },
            false,
        ) {
            tracing::error!("Failed to index series: {}", e);
        }

        Ok((StatusCode::CREATED, Json(record)))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(put, path = "/{id}", tag = "Series", operation_id = "updateSeries",
            responses((status = StatusCode::OK, description = "Updated Series", body = Series))))]
    pub async fn update(
        Path(id): Path<i64>,
        repository: SeriesRepository,
        ebook_repo: EbookRepository,
        State(state): State<AppState>,
        Garde(Json(payload)): Garde<Json<UpdateSeries>>,
    ) -> ApiResult<impl IntoResponse> {
        let record = repository.update(id, payload).await?;

        if let Err(e) = state.search().index_series(
            SeriesShort {
                id: record.id,
                title: record.title.clone(),
            },
            true,
        ) {
            tracing::error!("Failed to index series: {}", e);
        }

        if let Err(e) = reindex_books(&ebook_repo, state.search(), DependentId::Series(id)).await {
            tracing::error!("Error reindexing ebooks based on series update: {e}");
        }

        Ok((StatusCode::OK, Json(record)))
    }

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(delete, path = "/{id}", tag = "Series", operation_id = "deleteSeries")
    )]
    pub async fn delete(
        Path(id): Path<i64>,
        repository: SeriesRepository,
        ebook_repo: EbookRepository,
        State(state): State<AppState>,
    ) -> ApiResult<impl IntoResponse> {
        repository.delete(id).await?;

        if let Err(e) = state.search().delete_series(id) {
            tracing::error!("Failed to delete in series index: {}", e);
        }

        if let Err(e) = reindex_books(&ebook_repo, state.search(), DependentId::Series(id)).await {
            tracing::error!("Error reindexing ebooks based on series update: {e}");
        }

        Ok((StatusCode::NO_CONTENT, ()))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{id}", delete(extra_crud_api::delete))
        .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/", post(extra_crud_api::create))
        .route("/{id}", put(extra_crud_api::update))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
        .route("/{id}/ebooks", get(extra_crud_api::list_ebooks))
}
