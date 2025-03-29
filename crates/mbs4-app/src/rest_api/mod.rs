pub mod language;

#[macro_export]
macro_rules! crud_api {
    ($repository:ty, $create_type:ty) => {
        crate::repository_from_request!($repository);
        pub mod crud_api {
            use super::*;
            use crate::error::ApiResult;
            use axum::{extract::Path, response::IntoResponse, Json};
            use axum_valid::Garde;
            use http::StatusCode;
            pub async fn create(
                repository: $repository,
                Garde(Json(payload)): Garde<Json<$create_type>>,
            ) -> ApiResult<impl IntoResponse> {
                let record = repository.create(payload).await?;

                Ok((StatusCode::CREATED, Json(record)))
            }

            pub async fn list(repository: $repository) -> ApiResult<impl IntoResponse> {
                let users = repository.list(100).await?;
                Ok((StatusCode::OK, Json(users)))
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
