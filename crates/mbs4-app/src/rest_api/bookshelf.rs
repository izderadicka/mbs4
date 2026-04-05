use axum::routing::{get, post, put};
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
        extract::{Path, Query, State},
        response::IntoResponse,
        Json,
    };
    use axum_valid::Garde;
    use http::StatusCode;
    use mbs4_dal::bookshelf::{
        Bookshelf, BookshelfRepository, CreateBookshelf, CreateBookshelfItem, UpdateBookshelf,
        UpdateBookshelfItem,
    };
    #[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
    use mbs4_dal::bookshelf::{BookshelfItemListing, BookshelfListing};
    use mbs4_types::claim::{ApiClaim, Authorization as _, Role};

    use crate::{
        error::{ApiError, ApiResult},
        rest_api::Paging,
        state::AppState,
    };

    #[derive(serde::Serialize)]
    #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
    pub struct BookshelfItemMutationResponse {
        pub id: i64,
    }

    async fn get_accessible_bookshelf(
        bookshelf_id: i64,
        api_user: ApiClaim,
        repo: &BookshelfRepository,
    ) -> ApiResult<Bookshelf> {
        let bookshelf = repo.get(bookshelf_id).await?;
        if !bookshelf.public && bookshelf.created_by != Some(api_user.sub) {
            return Err(ApiError::DeniedAccess(
                "You don't have access to this bookshelf".into(),
            ));
        }
        Ok(bookshelf)
    }

    async fn get_owned_bookshelf(
        bookshelf_id: i64,
        api_user: ApiClaim,
        repo: &BookshelfRepository,
    ) -> ApiResult<Bookshelf> {
        let bookshelf = repo.get(bookshelf_id).await?;
        if bookshelf.created_by != Some(api_user.sub) {
            return Err(ApiError::DeniedAccess(
                "You don't have access to modify this bookshelf".into(),
            ));
        }
        Ok(bookshelf)
    }

    #[cfg(feature = "openapi")]
    #[cfg_attr(feature = "openapi", derive(utoipa::OpenApi))]
    #[openapi(paths(
        list_mine,
        list_public,
        list_items,
        add_item,
        update_item,
        delete_item,
        get,
        create,
        update,
        delete
    ))]
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

    #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/{id}/items", tag = "Bookshelf", operation_id = "listBookshelfItems",
        params(Paging), responses((status = StatusCode::OK, description = "List paginated", body = crate::rest_api::Page<BookshelfItemListing>))))]
    pub async fn list_items(
        Path(bookshelf_id): Path<i64>,
        api_user: ApiClaim,
        repo: BookshelfRepository,
        State(state): State<AppState>,
        Garde(Query(paging)): Garde<Query<Paging>>,
    ) -> ApiResult<impl IntoResponse> {
        get_accessible_bookshelf(bookshelf_id, api_user, &repo).await?;
        let default_page_size: u32 = state.config().default_page_size;
        let page_size = paging.page_size(default_page_size);
        let listing_params = paging.into_listing_params(default_page_size)?;
        let batch = repo.list_items(bookshelf_id, listing_params).await?;
        Ok((
            StatusCode::OK,
            Json(crate::rest_api::Page::from_batch(batch, page_size)),
        ))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/{id}", tag = "Bookshelf", operation_id = "getBookshelf",
         responses((status = StatusCode::OK, description = "Get bookshelf", body = Bookshelf))))]
    pub async fn get(
        Path(bookshelf_id): Path<i64>,
        api_user: ApiClaim,
        repo: BookshelfRepository,
    ) -> ApiResult<impl IntoResponse> {
        let shelf = get_accessible_bookshelf(bookshelf_id, api_user, &repo).await?;
        Ok((StatusCode::OK, Json(shelf)))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "", tag = "Bookshelf", operation_id = "createBookshelf",
            responses((status = StatusCode::CREATED, description = "Created Bookshelf", body = Bookshelf))))]
    pub async fn create(
        repository: BookshelfRepository,
        api_user: ApiClaim,
        Garde(Json(mut payload)): Garde<Json<CreateBookshelf>>,
    ) -> ApiResult<impl IntoResponse> {
        payload.created_by = Some(api_user.sub);
        let record = repository.create(payload).await?;

        Ok((StatusCode::CREATED, Json(record)))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "/{id}/items", tag = "Bookshelf", operation_id = "addBookshelfItem",
            responses((status = StatusCode::CREATED, description = "Created Bookshelf item", body = BookshelfItemMutationResponse))))]
    pub async fn add_item(
        Path(bookshelf_id): Path<i64>,
        api_user: ApiClaim,
        repository: BookshelfRepository,
        Garde(Json(mut payload)): Garde<Json<CreateBookshelfItem>>,
    ) -> ApiResult<impl IntoResponse> {
        let user = api_user.sub.clone();
        let _shelf = get_owned_bookshelf(bookshelf_id, api_user, &repository).await?;
        payload.created_by = Some(user);
        let id = repository.add_item(bookshelf_id, payload).await?;

        Ok((
            StatusCode::CREATED,
            Json(BookshelfItemMutationResponse { id }),
        ))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(put, path = "/{id}", tag = "Bookshelf", operation_id = "updateBookshelf",
            responses((status = StatusCode::OK, description = "Updated Bookshelf", body = Bookshelf))))]
    pub async fn update(
        Path(id): Path<i64>,
        api_user: ApiClaim,
        repository: BookshelfRepository,
        Garde(Json(payload)): Garde<Json<UpdateBookshelf>>,
    ) -> ApiResult<impl IntoResponse> {
        let _shelf = get_owned_bookshelf(id, api_user, &repository).await?;
        let record = repository.update(id, payload).await?;

        Ok((StatusCode::OK, Json(record)))
    }

    #[cfg_attr(feature = "openapi",  utoipa::path(put, path = "/{id}/items/{item_id}", tag = "Bookshelf", operation_id = "updateBookshelfItem",
            responses((status = StatusCode::OK, description = "Updated Bookshelf item", body = BookshelfItemMutationResponse))))]
    pub async fn update_item(
        Path((bookshelf_id, item_id)): Path<(i64, i64)>,
        api_user: ApiClaim,
        repository: BookshelfRepository,
        Garde(Json(payload)): Garde<Json<UpdateBookshelfItem>>,
    ) -> ApiResult<impl IntoResponse> {
        let _shelf = get_owned_bookshelf(bookshelf_id, api_user, &repository).await?;
        if payload.id != item_id {
            return Err(ApiError::InvalidRequest(
                "Bookshelf item id mismatch".into(),
            ));
        }
        let id = repository.update_item(bookshelf_id, payload).await?;

        Ok((StatusCode::OK, Json(BookshelfItemMutationResponse { id })))
    }

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(
            delete,
            path = "/{id}",
            tag = "Bookshelf",
            operation_id = "deleteBookshelf"
        )
    )]
    pub async fn delete(
        Path(id): Path<i64>,
        api_user: ApiClaim,
        repository: BookshelfRepository,
    ) -> ApiResult<impl IntoResponse> {
        if !api_user.has_role(Role::Admin) {
            get_owned_bookshelf(id, api_user, &repository).await?;
        }
        repository.delete(id).await?;

        Ok((StatusCode::NO_CONTENT, ()))
    }

    #[cfg_attr(
        feature = "openapi",
        utoipa::path(
            delete,
            path = "/{id}/items/{item_id}",
            tag = "Bookshelf",
            operation_id = "deleteBookshelfItem"
        )
    )]
    pub async fn delete_item(
        Path((bookshelf_id, item_id)): Path<(i64, i64)>,
        api_user: ApiClaim,
        repository: BookshelfRepository,
    ) -> ApiResult<impl IntoResponse> {
        let _shelf = get_owned_bookshelf(bookshelf_id, api_user, &repository).await?;
        repository.remove_item(bookshelf_id, item_id).await?;

        Ok((StatusCode::NO_CONTENT, ()))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        // .route("/{id}", delete(crud_api_extra::delete))
        // .layer(RequiredRolesLayer::new([Role::Admin]))
        .route("/mine", get(crud_api_extra::list_mine))
        .route("/", post(crud_api_extra::create))
        .route("/{id}/items", post(crud_api_extra::add_item))
        .route(
            "/{id}/items/{item_id}",
            put(crud_api_extra::update_item).delete(crud_api_extra::delete_item),
        )
        .route(
            "/{id}",
            put(crud_api_extra::update).delete(crud_api_extra::delete),
        )
        .layer(RequiredRolesLayer::new([Role::Trusted, Role::Admin]))
        .route("/{id}/items", get(crud_api_extra::list_items))
        .route("/public", get(crud_api_extra::list_public))
        .route("/{id}", get(crud_api_extra::get))
}
