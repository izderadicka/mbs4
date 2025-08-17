use crate::{auth::token::RequiredRolesLayer, crud_api, publish_api_docs};
#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::author::{Author, AuthorRepository, AuthorShort, CreateAuthor, UpdateAuthor};
use mbs4_types::claim::Role;

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};

publish_api_docs!(extra_crud_api::list_ebooks);
crud_api!(Author);

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

    use crate::{error::ApiResult, rest_api::Paging, state::AppState};

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
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{id}", delete(crud_api::delete))
        .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/", post(crud_api::create))
        .route("/{id}", put(crud_api::update))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/", get(crud_api::list))
        .route("/count", get(crud_api::count))
        .route("/{id}", get(crud_api::get))
        .route("/{id}/ebooks", get(extra_crud_api::list_ebooks))
}
