use crate::{commands::Executor as _, config::CliConfig};
use anyhow::Result;

pub async fn run(config: CliConfig) -> Result<()> {
    config.command.run().await
}
