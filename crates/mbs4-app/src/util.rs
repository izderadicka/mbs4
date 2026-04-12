use std::future::Future;

use mbs4_store::{file_store::FileStore, Store, ValidPath};
use tracing::error;

pub async fn cleanup_file_on_error<T, E, Fut>(
    store: &FileStore,
    stored_path: ValidPath,
    operation: Fut,
) -> Result<T, E>
where
    Fut: Future<Output = Result<T, E>>,
{
    match operation.await {
        Ok(value) => Ok(value),
        Err(err) => {
            store
                .delete(&stored_path)
                .await
                .inspect_err(|delete_err| {
                    error!(
                        "Failed to clean up stored file after downstream error at {:?}: {}",
                        stored_path, delete_err
                    )
                })
                .ok();
            Err(err)
        }
    }
}
