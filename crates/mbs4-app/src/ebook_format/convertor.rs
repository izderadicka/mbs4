use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use crate::{
    ebook_format::{ConversionResult, ErrorResult, MetaResult},
    events::EventMessage,
};
use mbs4_dal::conversion::{CreateConversion, EbookConversion};
use mbs4_store::{file_store::FileStore, upload_path, Store, StorePrefix, ValidPath};
use mbs4_types::utils::file_ext;
use tracing::error;

pub struct MetadataRequest {
    pub operation_id: String,
    pub file_path: ValidPath,
    pub extract_cover: bool,
}

pub struct ConversionRequest {
    pub operation_id: String,
    pub file_path: ValidPath,
    pub to_ext: String,
    pub source_id: i64,
    pub user: String,
}

enum ConversionJob {
    ExtractMetadata(MetadataRequest),
    Convert(ConversionRequest),
}

#[derive(Clone)]
pub struct Convertor {
    inner: Arc<ConvertorInner>,
}

pub struct ConvertorInner {
    event_sender: tokio::sync::broadcast::Sender<EventMessage>,
    job_queue: tokio::sync::mpsc::Sender<ConversionJob>,
    store: FileStore,
    pool: mbs4_dal::Pool,
}

impl Convertor {
    pub fn new(
        event_sender: tokio::sync::broadcast::Sender<EventMessage>,
        store: FileStore,
        pool: mbs4_dal::Pool,
    ) -> Self {
        let (job_sender, job_receiver) = tokio::sync::mpsc::channel(1024);
        let inner = ConvertorInner {
            event_sender,
            job_queue: job_sender,
            store,
            pool,
        };

        let convertor = Self {
            inner: Arc::new(inner),
        };
        convertor.start_main_loop(job_receiver);
        convertor
    }

    pub async fn extract_meta(&self, request: MetadataRequest) {
        self.inner
            .job_queue
            .send(ConversionJob::ExtractMetadata(request))
            .await
            .inspect_err(|_| error!("Convertor queue unexpectedly closed"))
            .ok();
    }

    pub async fn convert(&self, request: ConversionRequest) {
        self.inner
            .job_queue
            .send(ConversionJob::Convert(request))
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
                let inner = inner.clone();
                match job {
                    ConversionJob::ExtractMetadata(req) => {
                        tokio::spawn(async move {
                            inner.extract_meta(req).await;
                            drop(permit);
                        });
                    }
                    ConversionJob::Convert(req) => {
                        tokio::spawn(async move {
                            inner.convert(req).await;
                            drop(permit);
                        });
                    }
                }
            }
        });
    }
}

impl ConvertorInner {
    fn send_error(&self, operation_id: String, error: impl std::fmt::Display) {
        let error_result = ErrorResult {
            operation_id,
            created: time::OffsetDateTime::now_utc(),
            error: error.to_string(),
        };
        let event = EventMessage::message("extract_meta_error", error_result);
        self.event_sender.send(event).unwrap();
    }

    async fn extract_meta(self: Arc<Self>, request: MetadataRequest) {
        let MetadataRequest {
            operation_id,
            file_path,
            extract_cover,
        } = request;
        let local_path = self
            .store
            .local_path(&file_path)
            .expect("Current implementation always provide path");
        //TODO: case to download, if cannot get local path
        let meta_result = mbs4_calibre::extract_metadata(&local_path, extract_cover).await;
        match meta_result {
            Ok(mut meta) => {
                if let Some(cover_file) = meta.cover_file.take() {
                    async fn import_cover(
                        store: &FileStore,
                        cover_file: &str,
                    ) -> anyhow::Result<ValidPath> {
                        let cover_path = Path::new(&cover_file);
                        let ext = file_ext(cover_path)
                            .ok_or_else(|| anyhow::anyhow!("Invalid extension"))?;
                        let to_path = upload_path(&ext)?;
                        let import_path = store
                            .import_file(Path::new(&cover_file), &to_path, true)
                            .await?;
                        Ok(import_path)
                    }
                    match import_cover(&self.store, &cover_file).await {
                        Ok(path) => {
                            meta.cover_file = Some(
                                path.without_prefix(mbs4_store::StorePrefix::Upload)
                                    .unwrap() // save as we created on this prefix above
                                    .into(),
                            )
                        }
                        Err(e) => error!("Error when processing cover: {e}"),
                    }
                }

                let result = MetaResult {
                    operation_id,
                    created: time::OffsetDateTime::now_utc(),
                    metadata: meta,
                };
                let event = EventMessage::message("extract_meta", result);
                self.event_sender.send(event).unwrap();
            }
            Err(e) => self.send_error(operation_id, e),
        }
    }

    async fn convert(self: Arc<Self>, request: ConversionRequest) {
        let ConversionRequest {
            operation_id,
            file_path,
            to_ext,
            source_id,
            user,
        } = request;
        let local_path = self
            .store
            .local_path(&file_path)
            .expect("Current implementation always provides path");
        let meta_result = mbs4_calibre::convert(&local_path, &to_ext).await;

        match meta_result {
            Ok(converted_file) => {
                match self
                    .process_converted_file(converted_file, source_id, to_ext, user)
                    .await
                {
                    Ok(conversion) => {
                        let result = ConversionResult {
                            operation_id,
                            created: time::OffsetDateTime::now_utc(),
                            conversion,
                        };
                        let event = EventMessage::message("convert", result);
                        self.event_sender.send(event).unwrap();
                    }
                    Err(e) => self.send_error(operation_id, e),
                }
            }
            Err(e) => self.send_error(operation_id, e),
        }
    }

    async fn process_converted_file(
        &self,
        converted_file: PathBuf,
        source_id: i64,
        to_ext: String,
        user: String,
    ) -> anyhow::Result<mbs4_dal::conversion::EbookConversion> {
        // let mut tr = self.pool.begin().await?;
        let source = mbs4_dal::source::SourceRepository::new(self.pool.clone())
            .get(source_id)
            .await?;
        let ebook = mbs4_dal::ebook::EbookRepository::new(self.pool.clone())
            .get(source.ebook_id)
            .await?;
        let format_repository = mbs4_dal::format::FormatRepository::new(self.pool.clone());
        let format = format_repository.get_by_extension(&to_ext).await?;

        let naming = ebook.naming_meta();
        let ext = file_ext(&converted_file)
            .ok_or_else(|| anyhow::anyhow!("converted file is missing extension"))?;
        let final_path = naming.norm_file_name(&ext);
        let final_path = ValidPath::new(final_path)?.with_prefix(StorePrefix::Conversions);

        let new_path = self
            .store
            .import_file(&converted_file, &final_path, true)
            .await?;
        let new_path = new_path.without_prefix(StorePrefix::Conversions).unwrap();

        let create_conversion_result = CreateConversion {
            location: new_path.into(),
            source_id,
            format_id: format.id,
            batch_id: None,
            created_by: Some(user),
        };

        let conversion = mbs4_dal::conversion::ConversionRepository::new(self.pool.clone())
            .create(create_conversion_result)
            .await?;

        let source_format = mbs4_dal::format::FormatRepository::new(self.pool.clone())
            .get(source.format_id)
            .await?;

        Ok(EbookConversion {
            id: conversion.id,
            location: conversion.location,
            source_id: conversion.source_id,
            ebook_id: source.ebook_id,
            batch_id: conversion.batch_id,
            source_format_name: source_format.name,
            source_format_extension: source_format.extension,
            format_name: format.name,
            format_extension: format.extension,
            created_by: conversion.created_by,
            created: conversion.created,
        })
    }
}
