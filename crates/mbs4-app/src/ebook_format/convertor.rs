use std::{
    path::Path,
    sync::{Arc, LazyLock},
};

use crate::{ebook_format::MetaResult, events::EventMessage};
use mbs4_store::{file_store::FileStore, upload_path, Store, ValidPath};
use tracing::error;

pub enum ConversionJob {
    ExtractMetadata {
        operation_id: String,
        file_path: ValidPath,
        extract_cover: bool,
    },
}

#[derive(Clone)]
pub struct Convertor {
    inner: Arc<ConvertorInner>,
}

pub struct ConvertorInner {
    event_sender: tokio::sync::broadcast::Sender<EventMessage>,
    job_queue: tokio::sync::mpsc::Sender<ConversionJob>,
    store: FileStore,
}

impl Convertor {
    pub fn new(
        event_sender: tokio::sync::broadcast::Sender<EventMessage>,
        store: FileStore,
    ) -> Self {
        let (job_sender, job_receiver) = tokio::sync::mpsc::channel(1024);
        let inner = ConvertorInner {
            event_sender,
            job_queue: job_sender,
            store,
        };

        let convertor = Self {
            inner: Arc::new(inner),
        };
        convertor.start_main_loop(job_receiver);
        convertor
    }

    pub async fn extract_meta(&self, operation_id: String, file_path: ValidPath) {
        self.inner
            .job_queue
            .send(ConversionJob::ExtractMetadata {
                operation_id,
                file_path,
                extract_cover: true,
            })
            .await
            .inspect_err(|_| error!("Convertor queue unexpectedly closed"))
            .ok();
    }
}

static CONVERSION_LIMITS: LazyLock<tokio::sync::Semaphore> = LazyLock::new(|| {
    let num_cpus = num_cpus::get();
    tokio::sync::Semaphore::new(num_cpus)
});

impl Convertor {
    fn start_main_loop(&self, mut job_receiver: tokio::sync::mpsc::Receiver<ConversionJob>) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            while let Some(job) = job_receiver.recv().await {
                let permit = CONVERSION_LIMITS.acquire().await.unwrap(); // Safe - we never close
                match job {
                    ConversionJob::ExtractMetadata {
                        operation_id,
                        file_path,
                        extract_cover,
                    } => {
                        let inner = inner.clone();
                        tokio::spawn(async move {
                            inner
                                .extract_meta(operation_id, file_path, extract_cover)
                                .await;
                            drop(permit);
                        });
                    }
                }
            }
        });
    }
}

impl ConvertorInner {
    async fn extract_meta(
        self: Arc<Self>,
        operation_id: String,
        file_path: ValidPath,
        extract_cover: bool,
    ) {
        let local_path = self
            .store
            .local_path(&file_path)
            .expect("Current implementation always provide path");
        //TODO: case to download, if cannot get local path
        let local_path = local_path.to_str().unwrap(); // this is save as we assume utf-8 fs
        let meta_result = mbs4_calibre::extract_metadata(local_path, extract_cover).await;
        match meta_result {
            Ok(mut meta) => {
                if let Some(cover_file) = meta.cover_file.take() {
                    async fn import_cover(
                        store: &FileStore,
                        cover_file: &str,
                    ) -> anyhow::Result<ValidPath> {
                        let cover_path = Path::new(&cover_file);
                        let ext = cover_path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .ok_or_else(|| anyhow::anyhow!("Invalid extension"))?;
                        let to_path = upload_path(ext)?;
                        let import_path = store
                            .import_file(Path::new(&cover_file), &to_path, true)
                            .await?;
                        Ok(import_path)
                    }
                    match import_cover(&self.store, &cover_file).await {
                        Ok(path) => meta.cover_file = Some(path.into()),
                        Err(e) => error!("Error when processing cover: {e}"),
                    }
                }

                let result = MetaResult {
                    operation_id,
                    created: time::OffsetDateTime::now_utc(),
                    success: true,
                    error: None,
                    metadata: Some(meta),
                };
                let event = EventMessage::message("extract_meta", result);
                self.event_sender.send(event).unwrap();
            }
            Err(e) => {
                let error_result = MetaResult {
                    operation_id,
                    created: time::OffsetDateTime::now_utc(),
                    success: false,
                    error: Some(e.to_string()),
                    metadata: None,
                };
                let event = EventMessage::message("extract_meta_error", error_result);
                self.event_sender.send(event).unwrap();
            }
        }
    }
}
