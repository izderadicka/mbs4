use std::{
    ffi::OsStr,
    fmt::Display,
    path::{Path, PathBuf, StripPrefixError},
    sync::Arc,
};

use bytes::Bytes;
use futures::{Stream, StreamExt as _, TryFutureExt as _, TryStreamExt as _, pin_mut};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use tokio::{fs, io, io::AsyncWriteExt as _, task::spawn_blocking};
use tokio_util::io::ReaderStream;
use tracing::{debug, error};

use super::{
    Store, StoreInfo, ValidPath,
    error::{StoreError, StoreResult},
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
/// This is legacy algorithm to match existing files
/// There is also notable problem with it, that there is  possibility of race condition
/// This is compensated later by using lock
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

async fn tmp_path(root: &Path, path: &Path) -> StoreResult<PathBuf> {
    let id = uuid::Uuid::new_v4().to_string();
    let tmp_ext = format!("{id}.tmp");
    let tmp_path = path.with_extension(&tmp_ext);
    let tmp_path = root.join(tmp_path);
    if let Some(parent) = tmp_path.parent() {
        let meta = fs::metadata(parent).await;
        match meta {
            Ok(meta) => {
                if !meta.is_dir() {
                    error!("Parent is not a directory: {parent:?}");
                    return Err(StoreError::InvalidPath);
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    fs::create_dir_all(parent).await?;
                } else {
                    error!("Failed to stat parent: {parent:?}: {e}");
                    return Err(e.into());
                }
            }
        }
    }
    Ok(tmp_path)
}

fn unique_path_sync(final_path: PathBuf) -> StoreResult<PathBuf> {
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
        Ok(res_path)
    }
}

async fn unique_path(root: &Path, path: &str) -> StoreResult<PathBuf> {
    let path = root.join(path);
    spawn_blocking(|| unique_path_sync(path)).await?
}

async fn cleanup<E: Display>(path: &Path, error: E) -> Result<(), E> {
    error!("Failed to store file to path{path:?}: {error}");
    if path.exists() {
        fs::remove_file(path)
            .await
            .map_err(|e| error!("Failed to remove file {path:?}: {e}"))
            .ok();
    }
    Err(error)
}

struct FileStoreInner {
    root: PathBuf,
    lock: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct FileStore {
    inner: Arc<FileStoreInner>,
}

impl FileStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(FileStoreInner {
                root: root.into(),
                lock: tokio::sync::Mutex::new(()),
            }),
        }
    }

    fn relative_path(&self, path: &impl AsRef<Path>) -> Result<ValidPath, StripPrefixError> {
        let relative_path = path.as_ref().strip_prefix(&self.inner.root)?; // this is safe as we used root to create path
        let final_path = relative_path.to_str().unwrap().to_string(); // this is save as we assume utf-8 fs and path was created from string
        Ok(ValidPath(final_path)) // as input was ValidPath we expect ValidPath
    }

    async fn copy_file(
        &self,
        src: &Path,
        to_path: &ValidPath,
        remove_src: bool,
    ) -> StoreResult<PathBuf> {
        let dst_dir = self.inner.root.clone();
        let tmp = spawn_blocking(move || NamedTempFile::new_in(dst_dir)).await??; // propagate join errors

        // copy bytes
        let mut in_f = fs::File::open(src).await?;
        // reopen the temp path with tokio so we can write async
        let tmp_path = tmp.path();
        let mut out_f = fs::OpenOptions::new().write(true).open(tmp_path).await?;
        io::copy(&mut in_f, &mut out_f).await?;
        out_f.sync_all().await?;

        // persist atomically (blocking; wrap again)
        let final_path = {
            let final_path = unique_path(&self.inner.root, to_path.as_ref()).await?;
            spawn_blocking({
                let tmp = tmp;
                let dst = final_path.clone();
                move || tmp.persist(dst).map(|_| ())
            })
            .await?
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.error))?;
            final_path
        };

        if remove_src {
            fs::remove_file(src).await?
        };

        Ok(final_path)
    }
}

impl Store for FileStore {
    async fn store_data(&self, path: &ValidPath, data: &[u8]) -> StoreResult<StoreInfo> {
        let (final_path, mut new_file) = {
            let _lock = self.inner.lock.lock().await;
            let final_path = unique_path(&self.inner.root, path.as_ref()).await?;
            let new_file = fs::File::create(&final_path).await?;
            (final_path, new_file)
        };
        new_file
            .write_all(data)
            .or_else(|e| cleanup(&final_path, e))
            .await?;
        new_file.flush().await?;
        let digest = Sha256::digest(data);
        let final_path = self.relative_path(&final_path).unwrap(); // this is safe as we used root to create final_path
        let size = data.len() as u64;
        Ok(StoreInfo {
            final_path,
            size,
            hash: hex(&digest),
        })
    }

    async fn store_data_overwrite(&self, path: &ValidPath, data: &[u8]) -> StoreResult<StoreInfo> {
        let final_path = self.inner.root.join(path.as_ref());
        let folder = final_path.parent().ok_or(StoreError::InvalidPath)?;
        if !folder.exists() {
            fs::create_dir_all(folder).await?;
        }
        let mut tmp_path = final_path.clone();
        tmp_path.add_extension("tmp");
        let mut new_file = fs::File::create(&tmp_path).await?;
        new_file
            .write_all(data)
            .or_else(|e| cleanup(&tmp_path, e))
            .await?;
        new_file.flush().await?;
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

    async fn store_stream<S, E>(&self, path: &ValidPath, stream: S) -> StoreResult<StoreInfo>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: Into<StoreError>,
    {
        let tmp_path = tmp_path(&self.inner.root, Path::new(path.as_ref())).await?;
        let mut file = fs::File::create(&tmp_path)
            .await
            .inspect_err(|e| error!("Failed to tmp file {tmp_path:?}: {e}"))?;
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
        let final_path = {
            let _lock = self.inner.lock.lock().await;
            let final_path = unique_path(&self.inner.root, path.as_ref()).await?;
            fs::rename(&tmp_path, &final_path).await?;
            final_path
        };
        debug!("Stored {size} bytes to {tmp_path:?} and will move to {final_path:?}");
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
        path: &ValidPath,
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

    async fn size(&self, path: &ValidPath) -> StoreResult<u64> {
        let final_path = self.inner.root.join(path.as_ref());
        let meta = fs::metadata(&final_path).await?;
        Ok(meta.len())
    }

    async fn rename(&self, from_path: &ValidPath, to_path: &ValidPath) -> StoreResult<ValidPath> {
        let full_path = self.inner.root.join(from_path.as_ref());
        if !fs::metadata(&full_path).await?.is_file() {
            error!("Path {full_path:?} is not a file");
            return Err(StoreError::InvalidPath);
        }

        let final_path = {
            let _lock = self.inner.lock.lock().await;
            let final_path = unique_path(&self.inner.root, to_path.as_ref()).await?;
            fs::rename(&full_path, &final_path).await?;
            final_path
        };
        debug!("Renamed to {final_path:?}");
        let final_path = self.relative_path(&final_path).unwrap(); // this is safe as we used root to create final_path
        Ok(final_path)
    }

    async fn import_file(
        &self,
        path: &std::path::Path,
        to_path: &ValidPath,
        move_file: bool,
    ) -> StoreResult<ValidPath> {
        let mut final_path = None;
        if move_file {
            let _lock = self.inner.lock.lock().await;
            let dest_path = unique_path(&self.inner.root, to_path.as_ref()).await?;
            match fs::rename(path, &dest_path).await {
                Ok(()) => {
                    debug!("Moved file to {dest_path:?}");
                    final_path = Some(dest_path)
                }
                Err(e) => {
                    let is_exdev = e.raw_os_error() == Some(libc::EXDEV);
                    if !is_exdev {
                        return Err(e.into());
                    } else {
                        debug!("destination is on different mount, copying file");
                    }
                }
            }
        }

        if !move_file || final_path.is_none() {
            final_path = Some(self.copy_file(path, to_path, move_file).await?);
        }

        if let Some(final_path) = final_path {
            let final_path = self.relative_path(&final_path).unwrap(); // this is safe as we used root to create final_path
            Ok(final_path)
        } else {
            unreachable!("Should have path or return earlier")
        }
    }

    fn local_path(&self, path: &ValidPath) -> Option<std::path::PathBuf> {
        Some(self.inner.root.join(path.as_ref()))
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
        let validated_path = ValidPath::new("usarna/kulisatna.txt").unwrap();
        let validated_path2 = validated_path.clone();
        let handle =
            tokio::spawn(async move { store2.store_data(&validated_path2, content).await });
        let res = handle.await.unwrap().unwrap();
        assert_eq!(res.size, 12);
        assert_eq!(res.final_path.as_ref(), "usarna/kulisatna.txt");
        let res_path = store.inner.root.join("usarna/kulisatna.txt");
        assert!(res_path.exists());

        assert_eq!(fs::read(res_path).await.unwrap(), content);
        let res2 = store.store_data(&validated_path, content).await.unwrap();
        assert_eq!(res2.final_path.as_ref(), "usarna/kulisatna(1).txt");
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

    #[tracing_test::traced_test]
    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn test_stream() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let chunks = data_generator(10);

        let store = FileStore::new(tmp_dir.path());
        let validated_path = ValidPath::new("binarni/data").unwrap();
        let res = store.store_stream(&validated_path, chunks).await.unwrap();
        assert_eq!(res.final_path.as_ref(), "binarni/data");
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
        let validated_path = ValidPath::new("binarni/data").unwrap();
        let store = FileStore::new(tmp_dir.path());
        let _res = store.store_stream(&validated_path, chunks).await.unwrap();
        let validated_path = ValidPath::new("binarni/data").unwrap();
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn test_rename() {
        let size_kb: u8 = 5;
        let size = size_kb as usize * 1024;
        let tmp_dir = tempfile::tempdir().unwrap();
        let chunks = data_generator(size_kb);
        let original_path = ValidPath::new("binarni/data").unwrap();
        let store = FileStore::new(tmp_dir.path());
        let _res = store.store_stream(&original_path, chunks).await.unwrap();
        let renamed_path = ValidPath::new("finalni/data.bin").unwrap();
        let res = store.rename(&original_path, &renamed_path).await.unwrap();
        assert_eq!(res.as_ref(), "finalni/data.bin");

        let mut stream = store.load_data(&renamed_path).await.unwrap();
        let mut data = Vec::with_capacity(size); // 5MB
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            data.extend_from_slice(&chunk);
        }
        assert_eq!(data.len(), size);
        let original = fs::read(tmp_dir.path().join("finalni/data.bin"))
            .await
            .unwrap();
        assert_eq!(data, original);

        let res = store.load_data(&original_path).await;
        assert!(res.is_err());
        if let Err(err) = res {
            assert!(matches!(err, StoreError::NotFound(_)));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn test_import() {
        let size_kb: u8 = 5;
        let size = size_kb as usize * 1024;
        let tmp_dir = tempfile::tempdir().unwrap();
        let chunks = data_generator(size_kb);
        tokio::pin! {
        let reader = tokio_util::io::StreamReader::new(
            chunks.map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
        );
        }
        let tmp_dir2 = tempfile::tempdir().unwrap();
        let external_file = tmp_dir.path().join("test_data");

        let mut f = fs::File::create(&external_file).await.unwrap();
        io::copy(&mut reader, &mut f).await.unwrap();

        let store = FileStore::new(tmp_dir2.path());
        let to_path = ValidPath::new("upload/data.bin").unwrap();
        let name = store
            .import_file(&external_file, &to_path, false)
            .await
            .unwrap();
        assert_eq!("upload/data.bin", name.as_ref());

        async fn load_data(store: &FileStore, to_path: &ValidPath) -> Vec<u8> {
            let mut stream = store.load_data(&to_path).await.unwrap();

            let mut data: Vec<u8> = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.unwrap();
                data.extend_from_slice(&chunk);
            }

            data
        }

        let data = load_data(&store, &to_path).await;

        assert_eq!(data.len(), size);
        let original = fs::read(tmp_dir.path().join("test_data")).await.unwrap();
        assert_eq!(data, original);

        let name = store
            .import_file(&external_file, &to_path, true)
            .await
            .unwrap();
        assert_eq!("upload/data(1).bin", name.as_ref());

        let data = load_data(&store, &ValidPath::new("upload/data(1).bin").unwrap()).await;
        assert_eq!(data.len(), size);
        assert_eq!(data, original);
    }
}
