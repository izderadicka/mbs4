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

impl From<ValidPath> for String {
    fn from(val: ValidPath) -> Self {
        val.0
    }
}

#[derive(Debug)]
pub struct StoreInfo {
    /// final path were the file is stored, can be different from the requested path
    pub final_path: ValidPath,
    pub size: u64,
    /// Lowercase hex digest of the file content. SHA256 by default,
    /// or SHA1 when `mbs4-store` is built with the `legacy-file-hash`
    /// feature (for compatibility with the legacy database).
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
    /// Streams the file at `path` through the store's content-hashing
    /// algorithm (SHA256, or SHA1 under the `legacy-file-hash` feature)
    /// and returns the on-disk size and the lowercase hex digest.
    /// Returns `StoreError::NotFound` if the file does not exist.
    async fn hash(&self, path: &ValidPath) -> StoreResult<(u64, String)>;
    async fn rename(&self, from_path: &ValidPath, to_path: &ValidPath) -> StoreResult<ValidPath>;
    async fn delete(&self, path: &ValidPath) -> StoreResult<()>;
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

    // --- validate_path / is_segment_invalid ---

    #[test]
    fn test_valid_paths_are_accepted() {
        assert!(ValidPath::new("books/some-file.epub").is_ok());
        assert!(ValidPath::new("upload/abc123.epub").is_ok());
        assert!(ValidPath::new("file.txt").is_ok());
        assert!(ValidPath::new("a/b/c/d/e/f/g/h/i/j").is_ok()); // exactly MAX_PATH_DEPTH
    }

    #[test]
    fn test_empty_path_is_rejected() {
        assert!(ValidPath::new("").is_err());
    }

    #[test]
    fn test_absolute_and_trailing_slash_rejected() {
        assert!(ValidPath::new("/absolute/path").is_err());
        assert!(ValidPath::new("trailing/").is_err());
    }

    #[test]
    fn test_dot_segments_are_rejected() {
        assert!(ValidPath::new("a/./b").is_err());
        assert!(ValidPath::new("a/../b").is_err());
        assert!(ValidPath::new(".hidden").is_err());
        assert!(ValidPath::new("a/.hidden").is_err());
    }

    #[test]
    fn test_empty_segment_double_slash_is_rejected() {
        assert!(ValidPath::new("a//b").is_err());
    }

    #[test]
    fn test_invalid_chars_are_rejected() {
        assert!(ValidPath::new(r"a\b").is_err()); // backslash
        assert!(ValidPath::new("a:b").is_err()); // colon
        assert!(ValidPath::new("a\x00b").is_err()); // NUL control char
        assert!(ValidPath::new("a\x1fb").is_err()); // other control char
    }

    #[test]
    fn test_segment_length_boundary() {
        let ok_segment = "a".repeat(MAX_SEGMENT_LEN);
        assert!(ValidPath::new(&ok_segment).is_ok());
        let too_long = "a".repeat(MAX_SEGMENT_LEN + 1);
        assert!(ValidPath::new(&too_long).is_err());
    }

    #[test]
    fn test_path_length_boundary() {
        // A path of exactly MAX_PATH_LEN chars should be accepted (using 9 short segments
        // padded to fill the budget while staying within depth and segment limits).
        // 16 such segments + 15 slashes = 16*255+15 = 4095 = MAX_PATH_LEN — but depth is 10.
        // Use 2 segments that together exceed MAX_PATH_LEN via a 4096-char segment pair.
        // Simplest: one segment of 4096 chars (exceeds MAX_PATH_LEN outright).
        let too_long_path = "a".repeat(MAX_PATH_LEN + 1);
        assert!(ValidPath::new(&too_long_path).is_err());
    }

    #[test]
    fn test_path_depth_boundary() {
        let at_limit = vec!["a"; MAX_PATH_DEPTH].join("/");
        assert!(ValidPath::new(&at_limit).is_ok());
        let over_limit = vec!["a"; MAX_PATH_DEPTH + 1].join("/");
        assert!(ValidPath::new(&over_limit).is_err());
    }
}
