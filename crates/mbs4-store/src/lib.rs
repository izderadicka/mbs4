#![allow(async_fn_in_trait)]
use std::str::FromStr;

use bytes::Bytes;
use error::{StoreError, StoreResult};
use futures::Stream;

pub mod error;
pub mod file_store;
use tracing::debug;

const UPLOAD_PATH_PREFIX: &str = "upload";
const BOOKS_PATH_PREFIX: &str = "books";
const ICONS_PATH_PREFIX: &str = "icons";
const CONVERSIONS_PATH_PREFIX: &str = "converted";

const MAX_PATH_LEN: usize = 4095;
const MAX_SEGMENT_LEN: usize = 255;
const MAX_PATH_DEPTH: usize = 10;
const PATH_INVALID_CHARS: &str = r#"/\:"#;

pub enum StorePrefix {
    Upload,
    Books,
    Icons,
    Conversions,
}

impl StorePrefix {
    pub fn as_str(&self) -> &'static str {
        match self {
            StorePrefix::Upload => UPLOAD_PATH_PREFIX,
            StorePrefix::Books => BOOKS_PATH_PREFIX,
            StorePrefix::Icons => ICONS_PATH_PREFIX,
            StorePrefix::Conversions => CONVERSIONS_PATH_PREFIX,
        }
    }
}

fn is_segment_invalid(s: &str) -> bool {
    s.is_empty()
        || s.starts_with(".")
        || s.len() > MAX_SEGMENT_LEN
        || s.chars()
            .any(|c| PATH_INVALID_CHARS.contains(c) || c.is_ascii_control())
}

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
    let invalid_path = segments.into_iter().any(is_segment_invalid);
    if invalid_path {
        Err(StoreError::InvalidPath)
    } else {
        Ok(())
    }
}

pub fn upload_path(ext: &str) -> StoreResult<ValidPath> {
    let id = uuid::Uuid::new_v4().to_string();
    let dest_path = format!("{id}.{ext}");
    let dest_path = ValidPath::new(dest_path)?.with_prefix(StorePrefix::Upload);
    Ok(dest_path)
}

/// relative path, utf8, validated not to escape root and use . segments and some special chars
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidPath(String);

impl ValidPath {
    pub fn new(path: impl Into<String>) -> StoreResult<Self> {
        let path = path.into();
        validate_path(path.as_str()).inspect_err(|_| debug!("Invalid path: {path}"))?;
        Ok(ValidPath(path))
    }
    pub fn with_prefix(self, prefix: StorePrefix) -> Self {
        ValidPath(format!("{}/{}", prefix.as_str(), self.0))
    }

    pub fn without_prefix(self, expected_prefix: StorePrefix) -> StoreResult<Self> {
        match self.0.split_once('/') {
            Some((prefix, path)) => {
                if prefix == expected_prefix.as_str() {
                    Ok(ValidPath(path.into()))
                } else {
                    Err(StoreError::InvalidPath)
                }
            }
            None => Err(StoreError::InvalidPath),
        }
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

#[derive(Debug)]
pub struct StoreInfo {
    /// final path were the file is stored, can be different from the requested path
    pub final_path: ValidPath,
    pub size: u64,
    /// SHA256 hash
    pub hash: String,
}

pub trait Store {
    async fn store_data(&self, path: &ValidPath, data: &[u8]) -> StoreResult<StoreInfo>;
    async fn store_data_overwrite(&self, path: &ValidPath, data: &[u8]) -> StoreResult<StoreInfo>;
    async fn store_stream<S, E>(&self, path: &ValidPath, stream: S) -> StoreResult<StoreInfo>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: Into<StoreError>;
    async fn import_file(
        &self,
        path: &std::path::Path,
        final_path: &ValidPath,
        move_file: bool,
    ) -> StoreResult<ValidPath>;
    async fn load_data(
        &self,
        path: &ValidPath,
    ) -> Result<impl Stream<Item = StoreResult<Bytes>> + 'static, StoreError>;
    async fn size(&self, path: &ValidPath) -> StoreResult<u64>;
    async fn rename(&self, from_path: &ValidPath, to_path: &ValidPath) -> StoreResult<ValidPath>;
    fn local_path(&self, path: &ValidPath) -> Option<std::path::PathBuf>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_path() {
        assert!(ValidPath::new("a/b/c").is_ok());
        assert!(ValidPath::new("a/b/c/").is_err());
        assert!(ValidPath::new("a/b/c/..").is_err());
    }

    #[test]
    fn test_prefix() {
        let original_path = ValidPath::new("abcd.txt").unwrap();
        let path = original_path.clone().with_prefix(StorePrefix::Upload);
        assert_eq!(path.as_ref(), "upload/abcd.txt");
        let final_path = path.without_prefix(StorePrefix::Upload).unwrap();
        assert_eq!(final_path, original_path);
    }
}
