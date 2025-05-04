use mbs4_dal::ebook::EbookRepository;

use crate::state::AppState;
#[allow(unused_imports)]
use axum::routing::{delete, get, post, put};
crate::repository_from_request!(EbookRepository);
mod crud_api {
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
    pub async fn list(
        repository: EbookRepository,
        Garde(Query(paging)): Garde<Query<Paging>>,
    ) -> ApiResult<impl IntoResponse> {
        debug!("Paging: {:#?}", paging);
        let listing_params = paging.into_listing_params(100)?;
        let users = repository.list(listing_params).await?;
        Ok((StatusCode::OK, Json(users)))
    }

    pub async fn get(
        Path(id): Path<i64>,
        repository: EbookRepository,
    ) -> ApiResult<impl IntoResponse> {
        let record = repository.get(id).await?;

        Ok((StatusCode::OK, Json(record)))
    }
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/", get(crud_api::list))
        .route("/{id}", get(crud_api::get))
}
