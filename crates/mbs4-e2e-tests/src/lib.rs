use std::path::Path;

use anyhow::{Result, anyhow};
use mbs4_server::config::{Parser, ServerConfig};
use rand::Rng as _;
use tempfile::TempDir;

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

pub fn test_config(test_name: &str, base_dir: &Path) -> Result<(ServerConfig, ConfigGuard)> {
    let tmp_data_dir = TempDir::with_prefix_in(format!("{}_", test_name), base_dir)?;
    let data_dir = tmp_data_dir.path().to_string_lossy().to_string();
    let port = random_port()?;
    let port = port.to_string();
    let base_url = format!("http://localhost:{}", port);
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
    ];
    let config = ServerConfig::try_parse_from(args)?;
    Ok((
        config,
        ConfigGuard {
            data_dir: tmp_data_dir,
        },
    ))
}
