#[macro_export]
macro_rules! api_read_only {
    ($entity:ty) => {
        #[cfg(feature = "openapi")]
        type EntityShort = paste::paste! {[<$entity Short>]};
        #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "", tag = stringify!($entity), operation_id = concat!("list", stringify!($entity)),
        params(Paging), responses((status = StatusCode::OK, description = "List paginated", body = crate::rest_api::Page<EntityShort>))))]
        pub async fn list(
            repository: EntityRepository,
            State(state): State<AppState>,
            Garde(Query(paging)): Garde<Query<Paging>>,
        ) -> ApiResult<impl IntoResponse> {
            let default_page_size: u32 = state.config().default_page_size;
            let page_size = paging.page_size(default_page_size);
            let listing_params = paging.into_listing_params(default_page_size)?;
            let batch = repository.list(listing_params).await?;
            Ok((
                StatusCode::OK,
                Json(crate::rest_api::Page::from_batch(batch, page_size)),
            ))
        }

        #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/all", tag = stringify!($entity), operation_id = concat!("listAll", stringify!($entity)),
        responses((status = StatusCode::OK, description = "List all (unpaginated, sorted by id, max limit applies)", body = Vec<EntityShort>))))]
        pub async fn list_all(repository: EntityRepository) -> ApiResult<impl IntoResponse> {
            let users = repository.list_all().await?;
            Ok((StatusCode::OK, Json(users)))
        }

        #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/count", tag = stringify!($entity), operation_id = concat!("count", stringify!($entity)),
        responses((status = StatusCode::OK, description = "Count", body = u64))))]
        pub async fn count(repository: EntityRepository) -> ApiResult<impl IntoResponse> {
            let count = repository.count().await?;
            Ok((StatusCode::OK, Json(count)))
        }

        #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/{id}", tag = stringify!($entity), operation_id = concat!("get", stringify!($entity)),
        responses((status = StatusCode::OK, description = "Get one", body = $entity))))]
        pub async fn get(
            Path(id): Path<i64>,
            repository: EntityRepository,
        ) -> ApiResult<impl IntoResponse> {
            let record = repository.get(id).await?;

            Ok((StatusCode::OK, Json(record)))
        }
    };
}

#[macro_export]
macro_rules! crud_api {
    ($entity:ty) => {
        type EntityRepository = paste::paste! {[<$entity Repository>]};
        crate::repository_from_request!(EntityRepository);
        pub mod crud_api {
            use super::*;
            use crate::error::ApiResult;
            use crate::rest_api::Paging;
            use crate::state::AppState;
            use axum::{
                extract::{Path, Query, State},
                response::IntoResponse,
                Json,
            };
            use axum_valid::Garde;
            use http::StatusCode;
            // use tracing::debug;

            type CreateEntity = paste::paste! {[<Create $entity>]};
            type UpdateEntity = paste::paste! {[<Update $entity>]};

            crate::api_read_only!($entity);


            #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "", tag = stringify!($entity), operation_id = concat!("create", stringify!($entity)),
            responses((status = StatusCode::CREATED, description = concat!("Created ", stringify!($entity)), body = $entity))))]
            pub async fn create(
                repository: EntityRepository,
                Garde(Json(payload)): Garde<Json<CreateEntity>>,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.create(payload).await?;

                Ok((StatusCode::CREATED, Json(record)))
            }

            #[cfg_attr(feature = "openapi",  utoipa::path(put, path = "/{id}", tag = stringify!($entity), operation_id = concat!("update", stringify!($entity)),
            responses((status = StatusCode::OK, description = concat!("Updated ", stringify!($entity)), body = $entity))))]
            pub async fn update(
                Path(id): Path<i64>,
                repository: EntityRepository,
                Garde(Json(payload)): Garde<Json<UpdateEntity>>,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.update(id, payload).await?;

                Ok((StatusCode::OK, Json(record)))
            }

            #[cfg_attr(feature = "openapi",  utoipa::path(delete, path = "/{id}", tag = stringify!($entity), operation_id = concat!("delete", stringify!($entity))))]
            pub async fn delete(
                Path(id): Path<i64>,
                repository: EntityRepository,
            ) -> ApiResult<impl IntoResponse> {
                repository.delete(id).await?;

                Ok((StatusCode::NO_CONTENT, ()))
            }

            #[cfg(feature = "openapi")]
            #[cfg_attr(feature = "openapi", derive(utoipa::OpenApi))]
            #[openapi(paths(list, list_all, count, get, delete, update, create))]
            struct ApiDocs;

            #[cfg(feature = "openapi")]
            pub(super) fn api_docs() -> utoipa::openapi::OpenApi {
                use utoipa::OpenApi as _;
                ApiDocs::openapi()
            }
        }
    };

    ($entity:ty, RO) => {
        type EntityRepository = paste::paste! {[<$entity Repository>]};
        crate::repository_from_request!(EntityRepository);
        pub mod crud_api {
            use super::*;
            use crate::error::ApiResult;
            use crate::rest_api::Paging;
            use crate::state::AppState;
            use axum::{
                extract::{Path, Query, State},
                response::IntoResponse,
                Json,
            };
            use axum_valid::Garde;
            use http::StatusCode;
            // use tracing::debug;

            crate::api_read_only!($entity);

            #[cfg(feature = "openapi")]
            #[cfg_attr(feature = "openapi", derive(utoipa::OpenApi))]
            #[openapi(paths(list, list_all, count, get))]
            struct ApiDocs;

            #[cfg(feature = "openapi")]
            pub(super) fn api_docs() -> utoipa::openapi::OpenApi {
                use utoipa::OpenApi as _;
                ApiDocs::openapi()
            }
        }
    };
}

#[macro_export]
macro_rules! publish_api_docs {
    () => {
        #[cfg(feature = "openapi")]
        pub fn api_docs() -> utoipa::openapi::OpenApi {
            crud_api::api_docs()
        }
    };
    ($($end_point:path),+) => {
        #[cfg(feature = "openapi")]
        #[derive(utoipa::OpenApi)]
        #[openapi(paths($($end_point),+))]
        struct ModuleDocs;

        #[cfg(feature = "openapi")]
        pub fn api_docs() -> utoipa::openapi::OpenApi {
            use utoipa::OpenApi as _;
            let docs = ModuleDocs::openapi();
            docs.merge_from(crud_api::api_docs())
        }
    };
}

#[macro_export]
macro_rules! value_router {
    () => {
        pub fn router() -> axum::Router<crate::state::AppState> {
            use crate::auth::token::RequiredRolesLayer;
            use axum::routing::{delete, get, post};
            use mbs4_types::claim::Role;
            axum::Router::new()
                .route("/", post(crud_api::create))
                .route("/{id}", delete(crud_api::delete).put(crud_api::update))
                .layer(RequiredRolesLayer::new([Role::Admin]))
                .route("/", get(crud_api::list))
                .route("/all", get(crud_api::list_all))
                .route("/count", get(crud_api::count))
                .route("/{id}", get(crud_api::get))
        }
    };
}
