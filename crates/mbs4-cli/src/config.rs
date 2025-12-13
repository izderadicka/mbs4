use clap::{Parser, Subcommand};

use crate::commands::{cleanup::CleanupCmd, upload::UploadCmd};

#[derive(Parser)]
#[command(
    version,
    about,
    long_about = "CLI for mbs4 - provides various commands to manage and interact with mbs4 server."
)]
pub struct CliConfig {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Cleanup(CleanupCmd),
    Upload(UploadCmd),
}

impl crate::commands::Executor for Command {
    async fn run(self) -> anyhow::Result<()> {
        match self {
            Command::Cleanup(cmd) => cmd.run().await,
            Command::Upload(cmd) => cmd.run().await,
        }
    }
}
