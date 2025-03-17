#![allow(async_fn_in_trait)]
use std::{future::Future, path::PathBuf};

use axum::{
    extract::{FromRequestParts, Path as UrlPath},
    RequestPartsExt as _,
};
use bytes::Bytes;
use error::{StoreError, StoreResult};
use futures::Stream;
use http::request::Parts;
use serde::{Deserialize, Serialize};

pub mod error;
pub mod file_store;
pub mod rest_api;
pub use rest_api::store_router;
use tracing::debug;

use crate::error::ApiError;

const MAX_PATH_LEN: usize = 4095;
const MAX_SEGMENT_LEN: usize = 255;
const MAX_PATH_DEPTH: usize = 10;
const PATH_INVALID_CHARS: &str = r#"/\:"#;
fn validate_path(path: &str) -> StoreResult<()> {
    if path.is_empty() {
        return Err(StoreError::InvalidPath);
    }
    if path.starts_with("/") || path.ends_with("/") {
        return Err(StoreError::InvalidPath);
    }
    if path.len() > MAX_PATH_LEN {
        return Err(StoreError::InvalidPath);
    }
    let segments = path.split('/').collect::<Vec<_>>();
    if segments.len() > MAX_PATH_DEPTH {
        return Err(StoreError::InvalidPath);
    }
    let invalid_path = segments.into_iter().any(|s| {
        s.is_empty()
            || s.starts_with(".")
            || s.len() > MAX_SEGMENT_LEN
            || s.chars()
                .any(|c| PATH_INVALID_CHARS.contains(c) || c.is_ascii_control())
    });
    if invalid_path {
        Err(StoreError::InvalidPath)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedPath(String);

impl ValidatedPath {
    pub fn new(path: impl Into<String>) -> StoreResult<Self> {
        let path = path.into();
        validate_path(path.as_str()).inspect_err(|_| debug!("Invalid path: {path}"))?;
        Ok(ValidatedPath(path))
    }
}

impl AsRef<str> for ValidatedPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<S> FromRequestParts<S> for ValidatedPath {
    type Rejection = ApiError;

    #[doc = " Perform the extraction."]
    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let UrlPath(path) = parts.extract::<UrlPath<String>>().await?;
            let validate_path = ValidatedPath::new(path)?;
            Ok(validate_path)
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct StoreInfo {
    /// final path were the file is stored, can be different from the requested path
    pub final_path: PathBuf,
    pub size: u64,
    /// SHA256 hash
    pub hash: String,
}

pub trait Store {
    async fn store_data(&self, path: &ValidatedPath, data: &[u8]) -> StoreResult<StoreInfo>;
    async fn store_stream<S, E>(&self, path: &ValidatedPath, stream: S) -> StoreResult<StoreInfo>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: Into<StoreError>;
    async fn load_data(
        &self,
        path: &ValidatedPath,
    ) -> Result<impl Stream<Item = StoreResult<Bytes>> + 'static, StoreError>;
    async fn size(&self, path: &ValidatedPath) -> StoreResult<u64>;
}
