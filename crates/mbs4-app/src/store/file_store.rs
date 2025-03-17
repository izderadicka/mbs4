use std::{
    ffi::OsStr,
    fmt::Display,
    path::{Path, PathBuf, StripPrefixError},
    sync::Arc,
};

use bytes::Bytes;
use futures::{pin_mut, Stream, StreamExt as _, TryFutureExt as _, TryStreamExt as _};
use sha2::{Digest, Sha256};
use tokio::{fs, io::AsyncWriteExt as _, task::spawn_blocking};
use tokio_util::io::ReaderStream;
use tracing::{debug, error};

use super::{
    error::{StoreError, StoreResult},
    Store, StoreInfo, ValidatedPath,
};

//from std
fn rsplit_file_at_dot(file: &OsStr) -> (Option<&OsStr>, Option<&OsStr>) {
    if file.as_encoded_bytes() == b".." {
        return (Some(file), None);
    }

    // The unsafety here stems from converting between &OsStr and &[u8]
    // and back. This is safe to do because (1) we only look at ASCII
    // contents of the encoding and (2) new &OsStr values are produced
    // only from ASCII-bounded slices of existing &OsStr values.
    let mut iter = file.as_encoded_bytes().rsplitn(2, |b| *b == b'.');
    let after = iter.next();
    let before = iter.next();
    if before == Some(b"") {
        (Some(file), None)
    } else {
        unsafe {
            (
                before.map(|s| OsStr::from_encoded_bytes_unchecked(s)),
                after.map(|s| OsStr::from_encoded_bytes_unchecked(s)),
            )
        }
    }
}

#[inline]
fn hex(bytes: &[u8]) -> String {
    base16ct::lower::encode_string(bytes)
}

const MAX_SAME_FILES: usize = 10;
/// This is legacy algorithm to match exitinng files
/// There is also notable problem with it, that there is  possibility of race condition
fn find_unique_path(path: &Path) -> StoreResult<PathBuf> {
    let (base_path, ext) = rsplit_file_at_dot(path.as_os_str());
    let new_path = if ext.is_some() && base_path.is_some() {
        base_path.unwrap()
    } else {
        path.as_os_str()
    };

    for i in 1..=MAX_SAME_FILES {
        let mod_suffix = format!("({}).", i);
        // Safe as we deal only with ASCII
        let s = unsafe { OsStr::from_encoded_bytes_unchecked(mod_suffix.as_bytes()) };
        let mut new_path = new_path.to_os_string();
        new_path.push(s);
        let mut new_path = PathBuf::from(new_path);
        if let Some(ext) = ext {
            new_path.set_extension(ext);
        }
        if !new_path.exists() {
            return Ok(new_path);
        }
    }

    Err(StoreError::PathConflict)
}

fn unique_path_sync(final_path: PathBuf) -> StoreResult<(PathBuf, PathBuf)> {
    if final_path.is_dir() {
        Err(StoreError::InvalidPath)
    } else {
        let res_path = if final_path.exists() {
            let new_path = find_unique_path(&final_path)?;
            new_path
        } else {
            if let Some(parent_dir) = final_path.parent() {
                if !parent_dir.exists() {
                    std::fs::create_dir_all(parent_dir)?;
                }
            }

            final_path
        };
        let temp_path = res_path.with_extension("tmp");
        Ok((res_path, temp_path))
    }
}

async fn unique_path(root: &Path, path: &str) -> StoreResult<(PathBuf, PathBuf)> {
    let path = root.join(path);
    spawn_blocking(|| unique_path_sync(path)).await?
}

async fn cleanup<E: Display>(path: &Path, error: E) -> Result<(), E> {
    error!("Failed to store file to tmp path{path:?}: {error}");
    fs::remove_file(path)
        .await
        .map_err(|e| error!("Failed to remove file {path:?}: {e}"))
        .ok();
    Err(error)
}

struct FileStoreInner {
    root: PathBuf,
}

#[derive(Clone)]
pub struct FileStore {
    inner: Arc<FileStoreInner>,
}

impl FileStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(FileStoreInner { root: root.into() }),
        }
    }

    fn relative_path(&self, path: &impl AsRef<Path>) -> Result<PathBuf, StripPrefixError> {
        path.as_ref()
            .strip_prefix(&self.inner.root)
            .map(|p| p.to_path_buf())
    }
}

impl Store for FileStore {
    async fn store_data(&self, path: &ValidatedPath, data: &[u8]) -> StoreResult<StoreInfo> {
        let (final_path, tmp_path) = unique_path(&self.inner.root, path.as_ref()).await?;
        fs::File::create(&tmp_path)
            .await?
            .write_all(data)
            .or_else(|e| cleanup(&tmp_path, e))
            .await?;
        fs::rename(&tmp_path, &final_path).await?;
        let digest = Sha256::digest(data);
        let final_path = self.relative_path(&final_path).unwrap(); // this is safe as we used root to create final_path
        let size = data.len() as u64;
        Ok(StoreInfo {
            final_path,
            size,
            hash: hex(&digest),
        })
    }

    async fn store_stream<S, E>(&self, path: &ValidatedPath, stream: S) -> StoreResult<StoreInfo>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: Into<StoreError>,
    {
        let (final_path, tmp_path) = unique_path(&self.inner.root, path.as_ref()).await?;
        let mut file = fs::File::create(&tmp_path).await?;
        let mut size = 0;
        pin_mut!(stream);
        let mut digester = Sha256::new();
        while let Some(chunk) = stream.next().await {
            match chunk.map_err(|e| e.into()) {
                Ok(chunk) => {
                    file.write_all(&chunk)
                        .or_else(|e| cleanup(&tmp_path, e))
                        .await?;
                    size = size + chunk.len() as u64;
                    digester.update(&chunk);
                }
                Err(e) => {
                    cleanup(&tmp_path, e).await?;
                    unreachable!()
                }
            }
        }
        file.flush().await?;
        debug!("Stored {size} bytes to {tmp_path:?} and will move to {final_path:?}");
        fs::rename(&tmp_path, &final_path).await?;
        let digest = digester.finalize();
        let final_path = self.relative_path(&final_path).unwrap();

        Ok(StoreInfo {
            final_path,
            size,
            hash: hex(&digest),
        })
    }

    async fn load_data(
        &self,
        path: &ValidatedPath,
    ) -> Result<impl Stream<Item = StoreResult<Bytes>> + 'static, StoreError> {
        let final_path = self.inner.root.join(path.as_ref());
        let file = fs::File::open(&final_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StoreError::NotFound(path.as_ref().to_string())
            } else {
                e.into()
            }
        })?;
        let stream = ReaderStream::new(file).map_err(StoreError::from);
        Ok(stream)
    }

    async fn size(&self, path: &ValidatedPath) -> StoreResult<u64> {
        let final_path = self.inner.root.join(path.as_ref());
        let meta = fs::metadata(&final_path).await?;
        Ok(meta.len())
    }
}

#[cfg(test)]
mod tests {
    use futures::stream::try_unfold;

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn test_store() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let content = b"neco tady je";
        let store = FileStore::new(tmp_dir.path());
        let store2 = store.clone();
        // test to move store to other thread
        let validated_path = ValidatedPath::new("usarna/kulisatna.txt").unwrap();
        let validated_path2 = validated_path.clone();
        let handle =
            tokio::spawn(async move { store2.store_data(&validated_path2, content).await });
        let res = handle.await.unwrap().unwrap();
        assert_eq!(res.size, 12);
        assert_eq!(res.final_path, Path::new("usarna/kulisatna.txt"));
        assert!(store.inner.root.join("usarna/kulisatna.txt").exists());
        assert_eq!(
            fs::read(store.inner.root.join("usarna/kulisatna.txt"))
                .await
                .unwrap(),
            content
        );
        let res2 = store.store_data(&validated_path, content).await.unwrap();
        assert_eq!(res2.final_path, Path::new("usarna/kulisatna(1).txt"));
        assert!(store.inner.root.join("usarna/kulisatna(1).txt").exists());
        assert_eq!(
            fs::read(store.inner.root.join("usarna/kulisatna(1).txt"))
                .await
                .unwrap(),
            content
        );
    }

    fn data_generator(size_kb: u8) -> impl Stream<Item = StoreResult<Bytes>> {
        try_unfold(size_kb, |mut count| async move {
            if count == 0 {
                Ok::<_, StoreError>(None)
            } else {
                let data = rand::random::<[u8; 1024]>();
                let data = data.to_vec();
                count -= 1;

                Ok(Some((Bytes::from(data), count)))
            }
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn test_stream() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let chunks = data_generator(10);

        let store = FileStore::new(tmp_dir.path());
        let validated_path = ValidatedPath::new("binarni/data").unwrap();
        let res = store.store_stream(&validated_path, chunks).await.unwrap();
        assert_eq!(res.final_path, Path::new("binarni/data"));
        assert_eq!(res.size, 10240);
        let file_path = store.inner.root.join("binarni/data");
        assert!(file_path.exists());
        let meta = file_path.metadata().unwrap();
        assert_eq!(meta.len(), 10240);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn test_load() {
        let size_kb: u8 = 100;
        let size = size_kb as usize * 1024;
        let tmp_dir = tempfile::tempdir().unwrap();
        let chunks = data_generator(size_kb);
        let validated_path = ValidatedPath::new("binarni/data").unwrap();
        let store = FileStore::new(tmp_dir.path());
        let _res = store.store_stream(&validated_path, chunks).await.unwrap();
        let validated_path = ValidatedPath::new("binarni/data").unwrap();
        let mut stream = store.load_data(&validated_path).await.unwrap();
        let mut data = Vec::with_capacity(size); // 5MB
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            data.extend_from_slice(&chunk);
        }
        assert_eq!(data.len(), size);
        let original = fs::read(tmp_dir.path().join("binarni/data")).await.unwrap();
        assert_eq!(data, original);
    }
}
