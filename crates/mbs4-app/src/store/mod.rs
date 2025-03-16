#![allow(async_fn_in_trait)]
use std::path::PathBuf;

use bytes::Bytes;
use error::{StoreError, StoreResult};
use futures::Stream;
use serde::{Deserialize, Serialize};

pub mod error;
pub mod file_store;
pub mod rest_api;
pub use rest_api::store_router;

#[derive(Debug, Serialize, Deserialize)]
pub struct StoreInfo {
    /// final path were the file is stored, can be different from the requested path
    pub final_path: PathBuf,
    pub size: u64,
    /// SHA256 hash
    pub hash: String,
}

pub trait Store {
    async fn store_data(&self, path: &str, data: &[u8]) -> StoreResult<StoreInfo>;
    async fn store_stream<S, E>(&self, path: &str, stream: S) -> StoreResult<StoreInfo>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: Into<StoreError>;
    async fn load_data(
        &self,
        path: &str,
    ) -> Result<impl Stream<Item = StoreResult<Bytes>> + 'static, StoreError>;
    async fn size(&self, path: &str) -> StoreResult<u64>;
}
