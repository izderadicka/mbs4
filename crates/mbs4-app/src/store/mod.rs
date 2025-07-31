#![allow(async_fn_in_trait)]
use std::{future::Future, str::FromStr};

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

const UPLOAD_PATH_PREFIX: &str = "upload";

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

/// relative path, utf8, validated not to escape root and use . segments and some special chars
#[derive(Debug, Clone)]
pub struct ValidPath(String);

impl ValidPath {
    pub fn new(path: impl Into<String>) -> StoreResult<Self> {
        let path = path.into();
        validate_path(path.as_str()).inspect_err(|_| debug!("Invalid path: {path}"))?;
        Ok(ValidPath(path))
    }
}

impl FromStr for ValidPath {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ValidPath::new(s)
    }
}

impl AsRef<str> for ValidPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Into<String> for ValidPath {
    fn into(self) -> String {
        self.0
    }
}

impl<S> FromRequestParts<S> for ValidPath {
    type Rejection = ApiError;

    #[doc = " Perform the extraction."]
    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let UrlPath(path) = parts.extract::<UrlPath<String>>().await?;
            let validate_path = ValidPath::new(path)?;
            Ok(validate_path)
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct StoreInfo {
    /// final path were the file is stored, can be different from the requested path
    pub final_path: String,
    pub size: u64,
    /// SHA256 hash
    pub hash: String,
}

pub trait Store {
    async fn store_data(&self, path: &ValidPath, data: &[u8]) -> StoreResult<StoreInfo>;
    async fn store_stream<S, E>(&self, path: &ValidPath, stream: S) -> StoreResult<StoreInfo>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: Into<StoreError>;
    async fn load_data(
        &self,
        path: &ValidPath,
    ) -> Result<impl Stream<Item = StoreResult<Bytes>> + 'static, StoreError>;
    async fn size(&self, path: &ValidPath) -> StoreResult<u64>;
    async fn rename(&self, from_path: &ValidPath, to_path: &ValidPath) -> StoreResult<()>;
}
