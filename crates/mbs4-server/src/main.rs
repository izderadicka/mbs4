use mbs4_server::{config::ServerConfig, error::Result, run::run};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = ServerConfig::load()?;

    run(args).await
}
