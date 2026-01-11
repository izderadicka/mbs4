use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::{Semaphore, SemaphorePermit};
use tracing::{debug, error};

use crate::run_command;

pub const SOFFICE_BINARY: &str = "soffice";

fn base_dir() -> PathBuf {
    std::env::temp_dir().join("mbs4-oo")
}

#[derive(Clone)]
pub struct OoPool {
    inner: Arc<Inner>,
}

struct Inner {
    env_paths: Vec<PathBuf>,
    sem: Semaphore,
    free: Mutex<Vec<usize>>,
}

pub struct OoEnvLease<'a> {
    inner: Arc<Inner>,
    idx: usize,
    pub path: PathBuf,
    _permit: SemaphorePermit<'a>, // dropping returns the semaphore permit automatically
}

impl OoPool {
    /// Create pool with `concurrency` distinct LibreOffice profile dirs.
    pub async fn new(mut concurrency: usize) -> Result<Self> {
        if concurrency == 0 {
            return Err(anyhow!("concurrency must be > 0"));
        }
        if concurrency > 32 {
            error!("concurrency > 32, clamping to 32");
            concurrency = 32;
        }

        let base_dir = base_dir();
        tokio::fs::create_dir_all(&base_dir)
            .await
            .with_context(|| format!("create_dir_all({})", base_dir.display()))?;

        let mut env_paths = Vec::with_capacity(concurrency);
        for i in 0..concurrency {
            let p = base_dir.join(format!("oohome{}", i));
            tokio::fs::create_dir_all(&p)
                .await
                .with_context(|| format!("create_dir_all({})", p.display()))?;
            env_paths.push(p);
        }

        let free = (0..concurrency).rev().collect::<Vec<_>>();

        Ok(Self {
            inner: Arc::new(Inner {
                env_paths,
                sem: Semaphore::new(concurrency),
                free: Mutex::new(free),
            }),
        })
    }

    /// Acquire a profile dir lease; released automatically on Drop.
    async fn acquire(&self) -> Result<OoEnvLease<'_>> {
        let permit = self.inner.sem.acquire().await?;

        let idx = {
            let mut free = self.inner.free.lock().unwrap();
            free.pop()
                .ok_or_else(|| anyhow!("no env available (inconsistent state)"))?
        };

        Ok(OoEnvLease {
            inner: self.inner.clone(),
            idx,
            path: self.inner.env_paths[idx].clone(),
            _permit: permit,
        })
    }

    /// Convert `input` using LibreOffice headless into `format`, returning output path.
    /// Uses pool to allow concurrent conversions safely.
    ///
    pub async fn convert_file(
        &self,
        input: &Path,
        format: &str,
        outdir: Option<&Path>,
    ) -> Result<PathBuf> {
        let outdir = outdir.unwrap_or_else(|| input.parent().unwrap_or_else(|| Path::new(".")));

        let stem = input
            .file_stem()
            .ok_or_else(|| anyhow!("input file has no stem: {}", input.display()))?;
        let out_file = outdir.join(format!("{}.{}", stem.to_string_lossy(), format));

        let env = self.acquire().await?;
        let user_install = format!("file://{}", env.path.display());

        let mut cmd = Command::new(SOFFICE_BINARY);
        cmd.arg("--headless")
            .arg("--nologo")
            .arg("--nodefault")
            .arg("--norestore")
            .arg(format!("-env:UserInstallation={}", user_install))
            .arg("--convert-to")
            .arg(format)
            .arg("--outdir")
            .arg(outdir)
            .arg(input);

        let output = run_command(&mut cmd, Duration::from_secs(240)).await?;
        debug!(
            "LibreOffice conversion output: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        debug!(
            "LibreOffice conversion stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Check file exists
        if tokio::fs::try_exists(&out_file).await.unwrap_or(false) {
            Ok(out_file)
        } else {
            error!(
                "LibreOffice conversion failed to create output: input={} format={} outdir={}",
                input.display(),
                format,
                outdir.display(),
            );
            Err(anyhow!(
                "conversion failed to create output: {} -> {} ",
                input.display(),
                out_file.display(),
            ))
        }
    }
}

impl<'a> Drop for OoEnvLease<'a> {
    fn drop(&mut self) {
        let mut free = self.inner.free.lock().unwrap();
        free.push(self.idx);
        // semaphore permit returns automatically by dropping _permit
    }
}

#[cfg(test)]
mod tests {
    use tracing::info;

    use super::*;

    #[tokio::test]
    async fn test_pool() {
        let pool = OoPool::new(1).await.unwrap();
        let lease1 = pool.acquire().await.unwrap();
        let has_dir = tokio::fs::try_exists(&lease1.path).await.unwrap();
        assert!(has_dir);
        let lease2 = tokio::time::timeout(Duration::from_millis(100), pool.acquire()).await;
        assert!(lease2.is_err());

        drop(lease1);
        let lease2 = pool.acquire().await.unwrap();
        let has_dir = tokio::fs::try_exists(&lease2.path).await.unwrap();
        assert!(has_dir);

        //delete all base dir:
        tokio::fs::remove_dir_all(base_dir()).await.unwrap();
    }

    #[tokio::test]
    #[ignore] // requires libreoffice
    async fn test_convert() {
        tracing_subscriber::fmt::try_init().ok();
        let pool = OoPool::new(1).await.unwrap();
        let path = "../../test-data/samples/Pruvodce.doc";
        let path_exists = tokio::fs::try_exists(Path::new(path)).await.unwrap();
        assert!(path_exists);
        let base_dir = base_dir();
        let out_dir = base_dir.join(format!("out-{}", uuid::Uuid::new_v4()));
        let converted = pool
            .convert_file(Path::new(path), "html", Some(&out_dir))
            .await
            .unwrap();
        assert!(converted.exists());
        info!("converted: {}", converted.display());
        //delete all base dir:
        tokio::fs::remove_dir_all(base_dir).await.unwrap();
    }
}
