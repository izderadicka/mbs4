use clap::{ArgGroup, Args, Parser};
use mbs4_store::StorePrefix;
use mbs4_types::config::BackendConfig;
use tokio::fs;

use crate::commands::Executor;

#[derive(Parser, Debug)]
pub struct CleanupCmd {
    #[command(flatten)]
    backend: BackendConfig,
    #[command(flatten)]
    work: WorkSelection,
}

#[derive(Args, Debug)]
#[command(
    group(
        ArgGroup::new("work")
            .required(true)
            .args(["uploads", "all"])
    )
)]
pub struct WorkSelection {
    #[arg(long, help = "Delete old files in upload directory")]
    uploads: bool,
    #[arg(long, help = "Do all cleanup tasks")]
    all: bool,
}

const CLEANUP_INTERVAL_DAYS: u64 = 7;

impl Executor for CleanupCmd {
    async fn run(self) -> anyhow::Result<()> {
        if self.work.uploads || self.work.all {
            let upload_dir = self.backend.files_dir().join(StorePrefix::Upload.as_str());
            let mut files = fs::read_dir(&upload_dir).await?;
            while let Some(file) = files.next_entry().await? {
                let metadata = file.metadata().await?;
                if metadata
                    .created()
                    .or_else(|_| metadata.modified())?
                    .elapsed()
                    .unwrap()
                    .as_secs()
                    > 60 * 60 * 24 * CLEANUP_INTERVAL_DAYS
                {
                    fs::remove_file(file.path()).await?;
                    println!("Deleted {:?}", file.path());
                }
            }
        }

        Ok(())
    }
}
