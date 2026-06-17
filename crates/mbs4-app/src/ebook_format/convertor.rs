use std::{
    collections::{HashMap, HashSet},
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use crate::{
    ebook_format::{
        batch::{BatchComplete, BatchItemOutcomeKind, BatchProgress},
        source_pick::pick_best_source,
        ConversionResult, ErrorResult, MetaResult,
    },
    events::EventMessage,
    util::cleanup_file_on_error,
};
use mbs4_dal::conversion::{CreateConversion, EbookConversion};
use mbs4_store::{file_store::FileStore, upload_path, Store, StorePrefix, ValidPath};
use mbs4_types::utils::file_ext;
use tracing::{error, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionOperation {
    Convert,
    MetaExtract,
}

impl ConversionOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Convert => "convert",
            Self::MetaExtract => "meta_extract",
        }
    }
}

pub struct ConversionEvent {
    pub operation: ConversionOperation,
    pub duration: Duration,
    pub success: bool,
}

pub trait ConversionObserver: Send + Sync {
    fn on_conversion(&self, event: &ConversionEvent);
}

pub struct NoopConversionObserver;

impl ConversionObserver for NoopConversionObserver {
    fn on_conversion(&self, _event: &ConversionEvent) {}
}

pub fn noop_conversion_observer() -> Arc<dyn ConversionObserver> {
    Arc::new(NoopConversionObserver)
}

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

pub struct BatchJobRequest {
    pub operation_id: String,
    pub batch_id: i64,
    pub target_format_id: i64,
    pub target_format_extension: String,
    pub ebook_ids: Vec<i64>,
    /// Ebooks above the batch cap; not processed, listed in the ZIP manifest.
    pub dropped_ebook_ids: Vec<i64>,
    pub user: String,
}

enum ConversionJob {
    ExtractMetadata(MetadataRequest),
    Convert(ConversionRequest),
    Batch(BatchJobRequest),
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
    calibre: mbs4_calibre::Calibre,
    observer: Arc<dyn ConversionObserver>,
}

impl Convertor {
    pub async fn new(
        event_sender: tokio::sync::broadcast::Sender<EventMessage>,
        store: FileStore,
        pool: mbs4_dal::Pool,
        observer: Arc<dyn ConversionObserver>,
    ) -> anyhow::Result<Self> {
        let (job_sender, job_receiver) = tokio::sync::mpsc::channel(1024);
        let num_cpus = num_cpus::get();
        let calibre = mbs4_calibre::Calibre::new(num_cpus).await?;
        let inner = ConvertorInner {
            event_sender,
            job_queue: job_sender,
            store,
            pool,
            calibre,
            observer,
        };

        let convertor = Self {
            inner: Arc::new(inner),
        };
        convertor.start_main_loop(job_receiver);
        Ok(convertor)
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

    pub async fn convert_batch(&self, request: BatchJobRequest) {
        self.inner
            .job_queue
            .send(ConversionJob::Batch(request))
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
                    ConversionJob::Batch(req) => {
                        // A batch can span many items and would unfairly hog
                        // the conversion pool if it held a permit for the
                        // whole run. Release the slot up front; each
                        // per-source conversion inside the batch acquires its
                        // own permit.
                        drop(permit);
                        tokio::spawn(async move {
                            inner.convert_batch(req).await;
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
        let started = Instant::now();
        let meta_result = self
            .calibre
            .extract_metadata(&local_path, extract_cover)
            .await;
        let duration = started.elapsed();
        self.observer.on_conversion(&ConversionEvent {
            operation: ConversionOperation::MetaExtract,
            duration,
            success: meta_result.is_ok(),
        });
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
        let started = Instant::now();
        let meta_result = self.calibre.convert(&local_path, &to_ext).await;
        let duration = started.elapsed();
        self.observer.on_conversion(&ConversionEvent {
            operation: ConversionOperation::Convert,
            duration,
            success: meta_result.is_ok(),
        });

        match meta_result {
            Ok(converted_file) => {
                match self
                    .process_converted_file(converted_file, source_id, to_ext, user, None)
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
        batch_id: Option<i64>,
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
        let stored_path = new_path.clone();
        let new_path = new_path.without_prefix(StorePrefix::Conversions).unwrap();

        let create_conversion_result = CreateConversion {
            location: new_path.into(),
            source_id,
            format_id: format.id,
            batch_id,
            synthetic: false,
            created_by: Some(user),
        };

        let conversion = cleanup_file_on_error(
            &self.store,
            stored_path,
            mbs4_dal::conversion::ConversionRepository::new(self.pool.clone())
                .create(create_conversion_result),
        )
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
            synthetic: conversion.synthetic,
            source_format_name: source_format.name,
            source_format_extension: source_format.extension,
            format_name: format.name,
            format_extension: format.extension,
            created_by: conversion.created_by,
            created: conversion.created,
        })
    }

    async fn convert_batch(self: Arc<Self>, request: BatchJobRequest) {
        let BatchJobRequest {
            operation_id,
            batch_id,
            target_format_id,
            target_format_extension,
            ebook_ids,
            dropped_ebook_ids,
            user,
        } = request;

        let source_repo = mbs4_dal::source::SourceRepository::new(self.pool.clone());
        let conv_repo = mbs4_dal::conversion::ConversionRepository::new(self.pool.clone());
        let ebook_repo = mbs4_dal::ebook::EbookRepository::new(self.pool.clone());

        let total = ebook_ids.len();
        let mut ok_count = 0usize;
        let mut reused_count = 0usize;
        let mut err_count = 0usize;
        let mut entries: Vec<ManifestEntry> = Vec::with_capacity(total);

        for (idx, ebook_id) in ebook_ids.iter().enumerate() {
            let done = idx + 1;
            let label = ebook_repo
                .get(*ebook_id)
                .await
                .map(|e| e.title)
                .unwrap_or_else(|_| format!("ebook#{ebook_id}"));

            let outcome = self
                .process_batch_item(
                    *ebook_id,
                    batch_id,
                    target_format_id,
                    &target_format_extension,
                    &user,
                    &source_repo,
                    &conv_repo,
                )
                .await;
            let (kind, error) = match outcome {
                BatchItemOutcome::Done(k) => (k, None),
                BatchItemOutcome::Failed(e) => (BatchItemOutcomeKind::Failed, Some(e)),
            };
            match kind {
                BatchItemOutcomeKind::Converted => ok_count += 1,
                BatchItemOutcomeKind::ReusedSource | BatchItemOutcomeKind::ReusedConversion => {
                    reused_count += 1
                }
                BatchItemOutcomeKind::Failed => err_count += 1,
            }
            entries.push(ManifestEntry {
                ebook_id: *ebook_id,
                label: label.clone(),
                outcome: kind,
                error: error.clone(),
            });
            self.send_batch_progress(BatchProgress {
                operation_id: operation_id.clone(),
                batch_id,
                done,
                total,
                ebook_id: *ebook_id,
                label,
                outcome: kind,
                error,
            });
        }

        let (zip_location, zip_error) = match self
            .build_and_store_batch_zip(batch_id, &entries, &dropped_ebook_ids)
            .await
        {
            Ok(loc) => (Some(loc), None),
            Err(e) => {
                error!(batch_id, "batch zip creation failed: {e}");
                (None, Some(e.to_string()))
            }
        };

        self.send_batch_complete(BatchComplete {
            operation_id,
            batch_id,
            total,
            ok: ok_count,
            reused: reused_count,
            failed: err_count,
            dropped: dropped_ebook_ids.len(),
            zip_location,
            zip_error,
        });
    }

    /// Build the result ZIP for `batch_id`: pull every conversion row for the
    /// batch, copy each referenced file in, append a `manifest.txt`, import
    /// the archive under `Conversions/batches/...`, and update
    /// `conversion_batch.zip_location`. Returns the prefix-stripped location.
    async fn build_and_store_batch_zip(
        self: &Arc<Self>,
        batch_id: i64,
        entries: &[ManifestEntry],
        dropped_ebook_ids: &[i64],
    ) -> anyhow::Result<String> {
        let conv_repo = mbs4_dal::conversion::ConversionRepository::new(self.pool.clone());
        let ebook_repo = mbs4_dal::ebook::EbookRepository::new(self.pool.clone());
        let batch_repo =
            mbs4_dal::conversion_batch::ConversionBatchRepository::new(self.pool.clone());

        let conversions = conv_repo.list_for_batch(batch_id).await?;
        let by_ebook: HashMap<i64, EbookConversion> =
            conversions.into_iter().map(|c| (c.ebook_id, c)).collect();

        // Resolve each successful entry to (zip entry name, local file path).
        // Skip silently when the source can't be located locally — log and
        // continue so one missing file doesn't abort the whole archive.
        let mut zip_inputs: Vec<(String, PathBuf)> = Vec::new();
        let mut used_names: HashSet<String> = HashSet::new();
        for entry in entries {
            if matches!(entry.outcome, BatchItemOutcomeKind::Failed) {
                continue;
            }
            let Some(c) = by_ebook.get(&entry.ebook_id) else {
                continue;
            };
            let ebook = ebook_repo.get(c.ebook_id).await?;
            // A synthetic row whose source already matches the target format
            // (path 1 in `process_batch_item`) stored the source's
            // Books-relative location verbatim. All other rows live under
            // `Conversions`.
            let prefix = if c.synthetic
                && c.source_format_extension
                    .eq_ignore_ascii_case(&c.format_extension)
            {
                StorePrefix::Books
            } else {
                StorePrefix::Conversions
            };
            let path = ValidPath::new(c.location.clone())?.with_prefix(prefix);
            let Some(local) = self.store.local_path(&path) else {
                warn!(
                    ebook_id = c.ebook_id,
                    "file for batch entry not accessible locally; skipping"
                );
                continue;
            };
            let base = ebook.naming_meta().norm_file_name_only(&c.format_extension);
            let name = unique_zip_name(&mut used_names, &base);
            zip_inputs.push((name, local));
        }

        let manifest_body = render_manifest(entries, dropped_ebook_ids);

        // ZipWriter is sync I/O; do the file I/O on the blocking pool.
        let now = time::OffsetDateTime::now_utc();
        let temp_path = tokio::task::spawn_blocking(move || -> anyhow::Result<PathBuf> {
            let temp = tempfile::Builder::new()
                .prefix("mbs4-batch-")
                .suffix(".zip")
                .tempfile()?;
            {
                let mut writer = zip::ZipWriter::new(temp.as_file());
                let zip_dt = zip::DateTime::from_time(now).unwrap_or_default();
                let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .last_modified_time(zip_dt);
                for (name, local) in zip_inputs {
                    writer.start_file(&name, opts)?;
                    let mut f = std::fs::File::open(&local)?;
                    std::io::copy(&mut f, &mut writer)?;
                }
                writer.start_file("manifest.txt", opts)?;
                writer.write_all(manifest_body.as_bytes())?;
                writer.finish()?;
            }
            // `keep` disables the on-drop cleanup so `import_file` can move
            // the file. If `import_file` fails after this point, the temp
            // file is left behind — acceptable given the failure is already
            // reported via `batch_complete.zip_error`.
            let (_file, path) = temp.keep()?;
            Ok(path)
        })
        .await??;

        let final_path = ValidPath::new(format!("batches/batch-{batch_id}.zip"))?
            .with_prefix(StorePrefix::Conversions);
        let stored = self
            .store
            .import_file(&temp_path, &final_path, true)
            .await?;
        let stored_relative: String = stored
            .without_prefix(StorePrefix::Conversions)
            .expect("import target was prefixed with Conversions above")
            .into();

        batch_repo
            .set_zip_location(batch_id, &stored_relative)
            .await?;

        Ok(stored_relative)
    }

    async fn process_batch_item(
        self: &Arc<Self>,
        ebook_id: i64,
        batch_id: i64,
        target_format_id: i64,
        target_format_extension: &str,
        user: &str,
        source_repo: &mbs4_dal::source::SourceRepository,
        conv_repo: &mbs4_dal::conversion::ConversionRepository,
    ) -> BatchItemOutcome {
        let sources = match source_repo.list_for_ebook(ebook_id).await {
            Ok(s) if s.is_empty() => return BatchItemOutcome::Failed("no sources".into()),
            Ok(s) => s,
            Err(e) => return BatchItemOutcome::Failed(e.to_string()),
        };

        // (1) A source is already in the target format — record a synthetic
        //     conversion pointing at the source's file.
        if let Some(s) = sources.iter().find(|s| {
            s.format_extension
                .eq_ignore_ascii_case(target_format_extension)
        }) {
            return match conv_repo
                .create(CreateConversion {
                    location: s.location.clone(),
                    source_id: s.id,
                    format_id: target_format_id,
                    batch_id: Some(batch_id),
                    synthetic: true,
                    created_by: Some(user.to_string()),
                })
                .await
            {
                Ok(_) => BatchItemOutcome::Done(BatchItemOutcomeKind::ReusedSource),
                Err(e) => BatchItemOutcome::Failed(e.to_string()),
            };
        }

        // (2) A prior non-synthetic conversion at the target format exists —
        //     point a synthetic row at its file.
        match conv_repo
            .find_existing_for_ebook(ebook_id, target_format_id)
            .await
        {
            Ok(Some(c)) => {
                return match conv_repo
                    .create(CreateConversion {
                        location: c.location,
                        source_id: c.source_id,
                        format_id: target_format_id,
                        batch_id: Some(batch_id),
                        synthetic: true,
                        created_by: Some(user.to_string()),
                    })
                    .await
                {
                    Ok(_) => BatchItemOutcome::Done(BatchItemOutcomeKind::ReusedConversion),
                    Err(e) => BatchItemOutcome::Failed(e.to_string()),
                };
            }
            Ok(None) => {}
            Err(e) => return BatchItemOutcome::Failed(e.to_string()),
        }

        // (3) Pick the best source and run a real conversion.
        let best = match pick_best_source(&sources) {
            Some(s) => s,
            None => return BatchItemOutcome::Failed("no convertible source".into()),
        };

        let file_path = match ValidPath::new(best.location.clone()) {
            Ok(p) => p.with_prefix(StorePrefix::Books),
            Err(e) => return BatchItemOutcome::Failed(e.to_string()),
        };
        let local_path = match self.store.local_path(&file_path) {
            Some(p) => p,
            None => return BatchItemOutcome::Failed("source file not accessible".into()),
        };

        let permit = CONVERSION_LIMITS.acquire().await.unwrap(); // safe: semaphore is never closed
        let started = Instant::now();
        let conv_result = self
            .calibre
            .convert(&local_path, target_format_extension)
            .await;
        let duration = started.elapsed();
        drop(permit);
        self.observer.on_conversion(&ConversionEvent {
            operation: ConversionOperation::Convert,
            duration,
            success: conv_result.is_ok(),
        });

        match conv_result {
            Ok(converted_file) => match self
                .process_converted_file(
                    converted_file,
                    best.id,
                    target_format_extension.to_string(),
                    user.to_string(),
                    Some(batch_id),
                )
                .await
            {
                Ok(_) => BatchItemOutcome::Done(BatchItemOutcomeKind::Converted),
                Err(e) => BatchItemOutcome::Failed(e.to_string()),
            },
            Err(e) => BatchItemOutcome::Failed(e.to_string()),
        }
    }

    fn send_batch_progress(&self, progress: BatchProgress) {
        let event = EventMessage::message("batch_progress", progress);
        let _ = self.event_sender.send(event);
    }

    fn send_batch_complete(&self, complete: BatchComplete) {
        let event = EventMessage::message("batch_complete", complete);
        let _ = self.event_sender.send(event);
    }
}

enum BatchItemOutcome {
    Done(BatchItemOutcomeKind),
    Failed(String),
}

struct ManifestEntry {
    ebook_id: i64,
    label: String,
    outcome: BatchItemOutcomeKind,
    error: Option<String>,
}

fn render_manifest(entries: &[ManifestEntry], dropped: &[i64]) -> String {
    let mut out = String::new();
    for e in entries {
        let line = match e.outcome {
            BatchItemOutcomeKind::Converted => {
                format!("OK     | {} | id={}\n", e.label, e.ebook_id)
            }
            BatchItemOutcomeKind::ReusedSource => {
                format!("REUSED | source | {} | id={}\n", e.label, e.ebook_id)
            }
            BatchItemOutcomeKind::ReusedConversion => format!(
                "REUSED | prior conversion | {} | id={}\n",
                e.label, e.ebook_id
            ),
            BatchItemOutcomeKind::Failed => format!(
                "FAIL   | {} | id={} | {}\n",
                e.label,
                e.ebook_id,
                e.error.as_deref().unwrap_or("unknown error"),
            ),
        };
        out.push_str(&line);
    }
    for id in dropped {
        out.push_str(&format!("SKIPPED (batch size limit) | id={id}\n"));
    }
    out
}

/// Append `-2`, `-3`, ... before the extension when the candidate name has
/// already been used in the archive.
fn unique_zip_name(used: &mut HashSet<String>, candidate: &str) -> String {
    if used.insert(candidate.to_string()) {
        return candidate.to_string();
    }
    let (stem, ext) = match candidate.rsplit_once('.') {
        Some((s, e)) => (s, format!(".{e}")),
        None => (candidate, String::new()),
    };
    for n in 2..u32::MAX {
        let alt = format!("{stem}-{n}{ext}");
        if used.insert(alt.clone()) {
            return alt;
        }
    }
    // Astronomically unlikely; fall back to the original name.
    candidate.to_string()
}
