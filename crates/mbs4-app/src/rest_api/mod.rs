use crate::error::{ApiError, ApiResult};
use garde::Validate;
use mbs4_dal::{Batch, ListingParams};
use serde::Serialize;

pub mod author;
pub mod ebook;
pub mod format;
pub mod genre;
pub mod language;
pub mod series;
pub mod source;

#[derive(Debug, Clone, Validate, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi",into_params(parameter_in = Query))]
#[garde(allow_unvalidated)]
pub struct Paging {
    page: Option<u32>,
    #[garde(range(min = 1, max = 1000))]
    page_size: Option<u32>,
    #[garde(length(max = 255))]
    sort: Option<String>,
}

impl Paging {
    pub fn into_listing_params(self, default_page_size: u32) -> ApiResult<ListingParams> {
        let page = self.page.unwrap_or(1);
        let page_size = self.page_size.unwrap_or(default_page_size);
        let offset = (page - 1) * page_size;
        let limit = page_size;
        let order = self
            .sort
            .map(|orderings| {
                orderings
                    .split(',')
                    .map(|name| {
                        let (field_name, descending) = match name.trim() {
                            "" => {
                                return Err(ApiError::InvalidQuery(
                                    "Empty ordering name".to_string(),
                                ))
                            }
                            name if name.len() > 100 => {
                                return Err(ApiError::InvalidQuery(
                                    "Ordering name too long".to_string(),
                                ))
                            }
                            name if name.starts_with('+') => (&name[1..], false),
                            name if name.starts_with('-') => (&name[1..], true),
                            name => (name, false),
                        };

                        let order = if descending {
                            mbs4_dal::Order::Desc(field_name.to_string())
                        } else {
                            mbs4_dal::Order::Asc(field_name.to_string())
                        };

                        Ok(order)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;

        Ok(ListingParams {
            offset: offset.into(),
            limit: limit.into(),
            order,
        })
    }

    pub fn page_size(&self, default_page_size: u32) -> u32 {
        self.page_size.unwrap_or(default_page_size)
    }
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Page<T> {
    page: u32,
    page_size: u32,
    total: u32,
    rows: Vec<T>,
}

impl<T> Page<T>
where
    T: Serialize,
{
    pub fn try_from_batch(
        batch: Batch<T>,
        page_size: u32,
    ) -> Result<Self, std::num::TryFromIntError> {
        Ok(Self {
            page: u32::try_from(batch.offset)? / page_size + 1,
            page_size,
            total: u32::try_from(
                (u64::try_from(batch.total)? + page_size as u64 - 1) / page_size as u64,
            )?,
            rows: batch.rows,
        })
    }

    pub fn from_batch(batch: Batch<T>, page_size: u32) -> Self {
        Self::try_from_batch(batch, page_size).expect("Failed to convert batch to page")
        // As we control the batch, this should never fail
    }
}

#[macro_export]
macro_rules! api_read_only {
    ($entity:ty) => {
        #[cfg(feature = "openapi")]
        type EntityShort = paste::paste! {[<$entity Short>]};
        #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "", tag = stringify!($entity),
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

        #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/all", tag = stringify!($entity),
        responses((status = StatusCode::OK, description = "List all (unpaginated, sorted by id, max limit applies)", body = Vec<EntityShort>))))]
        pub async fn list_all(repository: EntityRepository) -> ApiResult<impl IntoResponse> {
            let users = repository.list_all().await?;
            Ok((StatusCode::OK, Json(users)))
        }

        #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/count", tag = stringify!($entity),
        responses((status = StatusCode::OK, description = "Count", body = u64))))]
        pub async fn count(repository: EntityRepository) -> ApiResult<impl IntoResponse> {
            let count = repository.count().await?;
            Ok((StatusCode::OK, Json(count)))
        }

        #[cfg_attr(feature = "openapi",  utoipa::path(get, path = "/{id}", tag = stringify!($entity),
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


            #[cfg_attr(feature = "openapi",  utoipa::path(post, path = "", tag = stringify!($entity),
            responses((status = StatusCode::CREATED, description = concat!("Created ", stringify!($entity)), body = $entity))))]
            pub async fn create(
                repository: EntityRepository,
                Garde(Json(payload)): Garde<Json<CreateEntity>>,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.create(payload).await?;

                Ok((StatusCode::CREATED, Json(record)))
            }

            #[cfg_attr(feature = "openapi",  utoipa::path(put, path = "/{id}", tag = stringify!($entity),
            responses((status = StatusCode::OK, description = concat!("Updated ", stringify!($entity)), body = $entity))))]
            pub async fn update(
                Path(id): Path<i64>,
                repository: EntityRepository,
                Garde(Json(payload)): Garde<Json<UpdateEntity>>,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.update(id, payload).await?;

                Ok((StatusCode::OK, Json(record)))
            }

            #[cfg_attr(feature = "openapi",  utoipa::path(delete, path = "/{id}", tag = stringify!($entity)))]
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
