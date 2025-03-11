use std::{env, path::Path};

use anyhow::{Result, anyhow};
use mbs4_server::{
    config::{Parser, ServerConfig},
    run::run,
};
use rand::Rng as _;
use tempfile::TempDir;
use tracing::debug;

pub async fn test_port(port: u16) -> Result<()> {
    let retries = 3;
    let mut wait_ms = 100;
    for i in 1..=retries {
        match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            Ok(_) => return Ok(()),
            Err(_) if i < retries => {
                debug!("Port {} is not available, retrying in {}ms", port, wait_ms);
                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                wait_ms *= 2;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }
    unreachable!()
}

fn random_port() -> Result<u16> {
    let mut rng = rand::rng();

    let mut retries = 3;
    while retries > 0 {
        let port: u16 = rng.random_range(3030..4030);
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse()?;
        match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(100)) {
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => return Ok(port),
            Err(_) => retries -= 1,
            Ok(_) => retries -= 1,
        }
    }

    Err(anyhow!("Could not find a free port"))
}

pub struct ConfigGuard {
    #[allow(dead_code)]
    data_dir: TempDir,
}

pub async fn prepare_env(test_name: &str) -> Result<(ServerConfig, ConfigGuard)> {
    let dir = std::env::current_dir()?;
    debug!("Current directory: {}", dir.display());
    let data_dir = dir.join("test-data");
    let (args, config_guard) = test_config(test_name, &data_dir)?;

    let pool = mbs4_dal::new_pool(&args.database_url).await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    Ok((args, config_guard))
}

pub async fn spawn_server(args: ServerConfig) -> Result<()> {
    let port = args.port;
    tokio::spawn(async move {
        println!("RUST_LOG is {}", env::var("RUST_LOG").unwrap_or_default());
        run(args).await.unwrap();
    });

    test_port(port).await
}

pub fn test_config(test_name: &str, base_dir: &Path) -> Result<(ServerConfig, ConfigGuard)> {
    let tmp_data_dir = TempDir::with_prefix_in(format!("{}_", test_name), base_dir)?;
    let data_dir = tmp_data_dir.path().to_string_lossy().to_string();
    let port = random_port()?;
    let port = port.to_string();
    let base_url = format!("http://localhost:{}", port);
    let db_url = format!(
        "sqlite://{}?mode=rwc",
        tmp_data_dir.path().join("test.db").display()
    );
    let args = &[
        "mbs4-e2e-tests",
        "--data-dir",
        &data_dir,
        "--port",
        &port,
        "--oidc-config",
        "../../test-data/oidc-config.toml",
        "--base-url",
        &base_url,
        "--database-url",
        &db_url,
    ];
    let config = ServerConfig::try_parse_from(args)?;
    Ok((
        config,
        ConfigGuard {
            data_dir: tmp_data_dir,
        },
    ))
}
