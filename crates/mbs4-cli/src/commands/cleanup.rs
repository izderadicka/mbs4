use std::time::{Duration, Instant};

use clap::{ArgGroup, Args, Parser};
use mbs4_dal::source::{Source, SourceRepository, UpdateSource};
use mbs4_store::{error::StoreError, file_store::FileStore, Store as _, StorePrefix, ValidPath};
use mbs4_types::config::BackendConfig;
use tokio::fs;
use tracing::{debug, error, info, warn};

use crate::commands::{create_source_repository, Executor};

#[derive(Parser, Debug)]
pub struct CleanupCmd {
    #[command(flatten)]
    backend: BackendConfig,
    #[command(flatten)]
    work: WorkSelection,
    #[command(flatten)]
    source_opts: SourceOptions,
    #[arg(
        long,
        help = "Do not modify the database or filesystem; just log intended changes"
    )]
    dry_run: bool,
}

#[derive(Args, Debug)]
#[command(
    group(
        ArgGroup::new("work")
            .required(true)
            .args(["uploads", "sources", "all"])
    )
)]
pub struct WorkSelection {
    #[arg(long, help = "Delete old files in upload directory")]
    uploads: bool,
    #[arg(
        long,
        help = "Scan source table; delete rows whose file is missing, log size mismatches; with --check-hash or --rehash also verify or rewrite hashes"
    )]
    sources: bool,
    #[arg(long, help = "Do all cleanup tasks (uploads + sources)")]
    all: bool,
}

#[derive(Args, Debug)]
#[command(
    group(
        ArgGroup::new("hash_action").args(["check_hash", "rehash"])
    )
)]
pub struct SourceOptions {
    #[arg(
        long,
        help = "For each source, compute hash and compare with stored hash. Read-only. Mutually exclusive with --rehash."
    )]
    check_hash: bool,
    #[arg(
        long,
        help = "For each source, recompute and rewrite the stored hash (also fixes size if it differs). Mutually exclusive with --check-hash."
    )]
    rehash: bool,
}

const CLEANUP_INTERVAL_DAYS: u64 = 7;
const SOURCE_PAGE_SIZE: i64 = 500;
const PROGRESS_INTERVAL: Duration = Duration::from_secs(30);

impl Executor for CleanupCmd {
    async fn run(self) -> anyhow::Result<()> {
        let do_uploads = self.work.uploads || self.work.all;
        let do_sources = self.work.sources || self.work.all;

        if do_uploads {
            self.cleanup_uploads().await?;
        }
        if do_sources {
            self.cleanup_sources().await?;
        }
        Ok(())
    }
}

impl CleanupCmd {
    async fn cleanup_uploads(&self) -> anyhow::Result<()> {
        let upload_dir = self.backend.files_dir().join(StorePrefix::Upload.as_str());

        let mut files = fs::read_dir(&upload_dir).await?;
        let mut count = 0u64;
        while let Some(file) = files.next_entry().await? {
            let metadata = file.metadata().await?;
            if metadata.is_file()
                && metadata
                    .created()
                    .or_else(|_| metadata.modified())?
                    .elapsed()
                    .unwrap()
                    .as_secs()
                    > 60 * 60 * 24 * CLEANUP_INTERVAL_DAYS
            {
                if self.dry_run {
                    info!("[dry-run] Would delete {:?} in uploads", file.path());
                } else {
                    fs::remove_file(file.path()).await?;
                    debug!("Deleted {:?} in uploads", file.path());
                }
                count += 1;
            }
        }
        info!(
            "{}{} stale upload(s)",
            if self.dry_run {
                "Would delete "
            } else {
                "Deleted "
            },
            count
        );
        Ok(())
    }

    async fn cleanup_sources(&self) -> anyhow::Result<()> {
        let repo = create_source_repository(&self.backend.database_url()).await?;
        let store = FileStore::new(self.backend.files_dir());
        let cleaner = SourceCleaner {
            repo: &repo,
            store: &store,
            dry_run: self.dry_run,
            check_hash: self.source_opts.check_hash,
            rehash: self.source_opts.rehash,
        };

        let mut counters = Counters::default();
        let mut last_progress = Instant::now();
        let mut last_id: i64 = 0;

        cleaner.dedup_locations(&mut counters).await?;

        loop {
            let page = repo.list_page(last_id, SOURCE_PAGE_SIZE).await?;
            if page.is_empty() {
                break;
            }
            for source in page.iter() {
                last_id = source.id;
                counters.seen += 1;
                cleaner.process(source, &mut counters).await;

                if last_progress.elapsed() >= PROGRESS_INTERVAL {
                    info!("{}sources progress: {}", cleaner.prefix(), counters);
                    last_progress = Instant::now();
                }
            }
        }

        info!("{}sources done: {}", cleaner.prefix(), counters);
        Ok(())
    }
}

struct SourceCleaner<'a> {
    repo: &'a SourceRepository,
    store: &'a FileStore,
    dry_run: bool,
    check_hash: bool,
    rehash: bool,
}

impl SourceCleaner<'_> {
    fn prefix(&self) -> &'static str {
        if self.dry_run {
            "[dry-run] "
        } else {
            ""
        }
    }

    /// Resolves rows that share a `location`: keeps the one matching the
    /// on-disk file, deletes the rest. Runs before the per-row scan so it
    /// sees clean data. A file that is missing is left for the scan's
    /// `delete_missing`; a file matching no row is left untouched.
    async fn dedup_locations(&self, c: &mut Counters) -> anyhow::Result<()> {
        for location in self.repo.duplicate_locations().await? {
            let rows = self.repo.find_all_by_location(&location).await?;
            if rows.len() < 2 {
                continue; // changed since the query
            }
            let vp = match ValidPath::new(location.as_str()) {
                Ok(v) => v.with_prefix(StorePrefix::Books),
                Err(e) => {
                    error!("duplicate location {location:?} invalid: {e}");
                    c.errors += 1;
                    continue;
                }
            };
            let (disk_size, disk_hash) = match self.store.hash(&vp).await {
                Ok(t) => t,
                Err(StoreError::NotFound(_)) => continue, // scan's delete_missing handles it
                Err(e) => {
                    error!("Error processing duplicate location {location:?}: {e}");
                    c.errors += 1;
                    continue;
                }
            };
            let keeper = rows
                .iter()
                .filter(|s| s.size as u64 == disk_size && s.hash == disk_hash)
                .min_by_key(|s| s.id);
            let Some(keeper_id) = keeper.map(|s| s.id) else {
                warn!(
                    "duplicate location {location:?}: {} rows, none match the on-disk file; leaving untouched",
                    rows.len()
                );
                continue;
            };
            for source in rows.iter().filter(|s| s.id != keeper_id) {
                if self.dry_run {
                    info!(
                        "[dry-run] would delete duplicate source id={} location={location:?} (keeping id={keeper_id})",
                        source.id
                    );
                    c.deleted += 1;
                    continue;
                }
                match self.repo.delete(source.id).await {
                    Ok(()) => {
                        info!(
                            "Deleted duplicate source id={} location={location:?} (keeping id={keeper_id})",
                            source.id
                        );
                        c.deleted += 1;
                    }
                    Err(e) => {
                        error!("Failed to delete duplicate source id={}: {e}", source.id);
                        c.errors += 1;
                    }
                }
            }
        }
        Ok(())
    }

    async fn process(&self, source: &Source, c: &mut Counters) {
        let vp = match ValidPath::new(source.location.as_str()) {
            Ok(v) => v.with_prefix(StorePrefix::Books),
            Err(e) => {
                error!(
                    "source id={} invalid location {:?}: {}",
                    source.id, source.location, e
                );
                c.errors += 1;
                return;
            }
        };

        if self.rehash {
            self.process_rehash(source, &vp, c).await;
        } else if self.check_hash {
            self.process_check_hash(source, &vp, c).await;
        } else {
            self.process_default(source, &vp, c).await;
        }
    }

    async fn process_default(&self, source: &Source, vp: &ValidPath, c: &mut Counters) {
        let Some(on_disk) = self.ensure_size(source, vp, c).await else {
            return;
        };
        if on_disk as i64 == source.size {
            return;
        }
        c.mismatched += 1;
        let Some((sz, hash)) = self.ensure_hash(source, vp, c).await else {
            return;
        };
        if hash == source.hash {
            info!(
                "{}source id={} size stale: {} (db) -> {} (disk), hash OK; fixing size",
                self.prefix(),
                source.id,
                source.size,
                on_disk
            );
            self.write_update(source, sz as i64, hash, c).await;
        } else {
            error!(
                "source id={} location={:?} CORRUPT: size {} (db) vs {} (disk), hash {} vs {}",
                source.id, source.location, source.size, on_disk, source.hash, hash
            );
            c.errors += 1;
        }
    }

    async fn process_check_hash(&self, source: &Source, vp: &ValidPath, c: &mut Counters) {
        let Some((on_disk_size, hash)) = self.ensure_hash(source, vp, c).await else {
            return;
        };
        let (size_diff, hash_diff) = note_diff(source, on_disk_size, &hash, c);
        if hash_diff {
            warn!(
                "source id={} hash mismatch: db={} disk={}",
                source.id, source.hash, hash
            );
        }
        if size_diff {
            warn!(
                "source id={} size mismatch: db={} disk={}",
                source.id, source.size, on_disk_size
            );
        }
    }

    async fn process_rehash(&self, source: &Source, vp: &ValidPath, c: &mut Counters) {
        let Some((on_disk_size, hash)) = self.ensure_hash(source, vp, c).await else {
            return;
        };
        let new_size = on_disk_size as i64;
        note_diff(source, on_disk_size, &hash, c);
        debug!(
            "{}source id={} rehash: hash {} -> {}, size {} -> {}",
            self.prefix(),
            source.id,
            source.hash,
            hash,
            source.size,
            new_size
        );
        self.write_update(source, new_size, hash, c).await;
    }

    async fn ensure_size(&self, source: &Source, vp: &ValidPath, c: &mut Counters) -> Option<u64> {
        match self.store.size(vp).await {
            Err(StoreError::NotFound(_)) => {
                self.delete_missing(source, c).await;
                None
            }
            Err(e) => {
                self.io_error(source, e, c);
                None
            }
            Ok(sz) => Some(sz),
        }
    }

    async fn ensure_hash(
        &self,
        source: &Source,
        vp: &ValidPath,
        c: &mut Counters,
    ) -> Option<(u64, String)> {
        match self.store.hash(vp).await {
            Err(StoreError::NotFound(_)) => {
                self.delete_missing(source, c).await;
                None
            }
            Err(e) => {
                self.io_error(source, e, c);
                None
            }
            Ok(t) => Some(t),
        }
    }

    async fn delete_missing(&self, source: &Source, c: &mut Counters) {
        if self.dry_run {
            info!(
                "[dry-run] would delete source id={} location={:?} (file missing)",
                source.id, source.location
            );
            c.deleted += 1;
            return;
        }
        match self.repo.delete(source.id).await {
            Ok(()) => {
                debug!("Deleted source id={} (file missing)", source.id);
                c.deleted += 1;
            }
            Err(e) => {
                error!("Failed to delete source id={}: {}", source.id, e);
                c.errors += 1;
            }
        }
    }

    async fn write_update(
        &self,
        source: &Source,
        new_size: i64,
        new_hash: String,
        c: &mut Counters,
    ) {
        if self.dry_run {
            c.updated += 1;
            return;
        }
        let payload = UpdateSource {
            id: source.id,
            version: source.version,
            location: source.location.clone(),
            ebook_id: source.ebook_id,
            format_id: source.format_id,
            size: new_size,
            hash: new_hash,
            quality: source.quality,
        };
        match self.repo.update(source.id, payload).await {
            Ok(_) => c.updated += 1,
            Err(e) => {
                warn!("Failed to update source id={}: {}", source.id, e);
                c.errors += 1;
            }
        }
    }

    fn io_error(&self, source: &Source, e: StoreError, c: &mut Counters) {
        error!(
            "source id={} location={:?}: {}",
            source.id, source.location, e
        );
        c.errors += 1;
    }
}

/// Compares on-disk (size, hash) to `source.size` / `source.hash`, bumps
/// `c.mismatched` once if either differs, and returns `(size_diff, hash_diff)`
/// so the caller can branch on which side disagreed.
fn note_diff(source: &Source, on_disk_size: u64, hash: &str, c: &mut Counters) -> (bool, bool) {
    let size_diff = on_disk_size as i64 != source.size;
    let hash_diff = hash != source.hash;
    if size_diff || hash_diff {
        c.mismatched += 1;
    }
    (size_diff, hash_diff)
}

#[derive(Default, Debug)]
struct Counters {
    seen: u64,
    deleted: u64,
    mismatched: u64,
    updated: u64,
    errors: u64,
}

impl std::fmt::Display for Counters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "seen={} deleted={} mismatched={} updated={} errors={}",
            self.seen, self.deleted, self.mismatched, self.updated, self.errors
        )
    }
}
