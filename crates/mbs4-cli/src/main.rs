use clap::Parser;
use mbs4_cli::{config::CliConfig, run::run};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let config = CliConfig::parse();

    run(config).await
}
