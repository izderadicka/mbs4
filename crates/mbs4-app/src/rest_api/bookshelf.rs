use axum::routing::get;
use mbs4_dal::bookshelf::BookshelfRepository;
use mbs4_types::claim::Role;

use crate::{auth::token::RequiredRolesLayer, state::AppState};

crate::repository_from_request!(BookshelfRepository);

#[cfg(feature = "openapi")]
pub fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    crud_api_extra::ApiDocs::openapi()
}

mod crud_api_extra {
    use axum::{
        extract::{Query, State},
        response::IntoResponse,
        Json,
    };
    use axum_valid::Garde;
    use http::StatusCode;
    use mbs4_dal::bookshelf::{BookshelfListing, BookshelfRepository};
    use mbs4_types::claim::ApiClaim;

    use crate::{error::ApiResult, rest_api::Paging, state::AppState};

    #[cfg(feature = "openapi")]
    #[cfg_attr(feature = "openapi", derive(utoipa::OpenApi))]
    #[openapi(paths(list_mine, list_public))]
    pub(super) struct ApiDocs;

    #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/mine", tag = "Bookshelf", operation_id = "listMyBookshelves",
        params(Paging), responses((status = StatusCode::OK, description = "List paginated", body = crate::rest_api::Page<BookshelfListing>))))]
    pub async fn list_mine(
        api_user: ApiClaim,
        repo: BookshelfRepository,
        State(state): State<AppState>,
        Garde(Query(paging)): Garde<Query<Paging>>,
    ) -> ApiResult<impl IntoResponse> {
        let default_page_size: u32 = state.config().default_page_size;
        let page_size = paging.page_size(default_page_size);
        let listing_params = paging.into_listing_params(default_page_size)?;
        let batch = repo.list_for_user(&api_user.sub, listing_params).await?;
        Ok((
            StatusCode::OK,
            Json(crate::rest_api::Page::from_batch(batch, page_size)),
        ))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/public", tag = "Bookshelf", operation_id = "listPublicBookshelves",
        params(Paging), responses((status = StatusCode::OK, description = "List paginated", body = crate::rest_api::Page<BookshelfListing>))))]
    pub async fn list_public(
        api_user: ApiClaim,
        repo: BookshelfRepository,
        State(state): State<AppState>,
        Garde(Query(paging)): Garde<Query<Paging>>,
    ) -> ApiResult<impl IntoResponse> {
        let default_page_size: u32 = state.config().default_page_size;
        let page_size = paging.page_size(default_page_size);
        let listing_params = paging.into_listing_params(default_page_size)?;
        let batch = repo.list_public(&api_user.sub, listing_params).await?;
        Ok((
            StatusCode::OK,
            Json(crate::rest_api::Page::from_batch(batch, page_size)),
        ))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        // .route("/{id}", delete(crud_api_extra::delete))
        // .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/mine", get(crud_api_extra::list_mine))
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/public", get(crud_api_extra::list_public))
}
