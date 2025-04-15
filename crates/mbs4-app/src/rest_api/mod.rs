use garde::Validate;
use mbs4_dal::ListingParams;

use crate::error::{ApiError, ApiResult};

pub mod author;
pub mod language;

#[derive(Debug, Clone, Validate, serde::Deserialize)]
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
}

#[macro_export]
macro_rules! crud_api {
    ($repository:ty, $create_type:ty, $update_type:ty) => {
        crate::repository_from_request!($repository);
        pub mod crud_api {
            use super::*;
            use crate::error::ApiResult;
            use crate::rest_api::Paging;
            use axum::{
                extract::{Path, Query},
                response::IntoResponse,
                Json,
            };
            use axum_valid::Garde;
            use http::StatusCode;
            use tracing::debug;
            pub async fn create(
                repository: $repository,
                Garde(Json(payload)): Garde<Json<$create_type>>,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.create(payload).await?;

                Ok((StatusCode::CREATED, Json(record)))
            }

            pub async fn list(
                repository: $repository,
                Garde(Query(paging)): Garde<Query<Paging>>,
            ) -> ApiResult<impl IntoResponse> {
                debug!("Paging: {:#?}", paging);
                let listing_params = paging.into_listing_params(100)?;
                let users = repository.list(listing_params).await?;
                Ok((StatusCode::OK, Json(users)))
            }

            pub async fn list_all(repository: $repository) -> ApiResult<impl IntoResponse> {
                let users = repository.list_all().await?;
                Ok((StatusCode::OK, Json(users)))
            }

            pub async fn count(repository: $repository) -> ApiResult<impl IntoResponse> {
                let count = repository.count().await?;
                Ok((StatusCode::OK, Json(count)))
            }

            pub async fn get(
                Path(id): Path<i64>,
                repository: $repository,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.get(id).await?;

                Ok((StatusCode::OK, Json(record)))
            }

            pub async fn update(
                Path(id): Path<i64>,
                repository: $repository,
                Garde(Json(payload)): Garde<Json<$update_type>>,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.update(id, payload).await?;

                Ok((StatusCode::OK, Json(record)))
            }

            pub async fn delete(
                Path(id): Path<i64>,
                repository: $repository,
            ) -> ApiResult<impl IntoResponse> {
                repository.delete(id).await?;

                Ok((StatusCode::NO_CONTENT, ()))
            }
        }
    };
}

#[macro_export]
macro_rules! crud_api_old {
    ($repository:ty, $create_type:ty) => {
        crate::repository_from_request!($repository);
        pub mod crud_api {
            use super::*;
            use crate::error::ApiResult;
            use crate::rest_api::Paging;
            use axum::{
                extract::{Path, Query},
                response::IntoResponse,
                Json,
            };
            use axum_valid::Garde;
            use http::StatusCode;
            use tracing::debug;
            pub async fn create(
                repository: $repository,
                Garde(Json(payload)): Garde<Json<$create_type>>,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.create(payload).await?;

                Ok((StatusCode::CREATED, Json(record)))
            }

            pub async fn list(
                repository: $repository,
                Garde(Query(paging)): Garde<Query<Paging>>,
            ) -> ApiResult<impl IntoResponse> {
                debug!("Paging: {:#?}", paging);
                let listing_params = paging.into_listing_params(100)?;
                let users = repository.list(listing_params).await?;
                Ok((StatusCode::OK, Json(users)))
            }

            pub async fn list_all(repository: $repository) -> ApiResult<impl IntoResponse> {
                let users = repository.list_all().await?;
                Ok((StatusCode::OK, Json(users)))
            }

            pub async fn count(repository: $repository) -> ApiResult<impl IntoResponse> {
                let count = repository.count().await?;
                Ok((StatusCode::OK, Json(count)))
            }

            pub async fn get(
                Path(id): Path<i64>,
                repository: $repository,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.get(id).await?;

                Ok((StatusCode::OK, Json(record)))
            }

            pub async fn update(
                Path(id): Path<i64>,
                repository: $repository,
                Garde(Json(payload)): Garde<Json<$create_type>>,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.update(id, payload).await?;

                Ok((StatusCode::OK, Json(record)))
            }

            pub async fn delete(
                Path(id): Path<i64>,
                repository: $repository,
            ) -> ApiResult<impl IntoResponse> {
                repository.delete(id).await?;

                Ok((StatusCode::NO_CONTENT, ()))
            }
        }
    };
}
