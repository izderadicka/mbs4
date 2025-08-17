#![allow(async_fn_in_trait)]
use std::future::Future;

use axum::{
    extract::{FromRequestParts, Path as UrlPath},
    RequestPartsExt as _,
};
use http::request::Parts;

pub mod rest_api;
use mbs4_store::ValidPath;
pub use rest_api::router;

use crate::{error::ApiError, state::AppState};

impl FromRequestParts<AppState> for ValidPath {
    type Rejection = ApiError;

    #[doc = " Perform the extraction."]
    fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let UrlPath(path) = parts.extract::<UrlPath<String>>().await?;
            let validate_path = ValidPath::new(path)?;
            Ok(validate_path)
        }
    }
}
