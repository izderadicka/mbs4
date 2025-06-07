use std::{env, path::Path};

use anyhow::{Result, anyhow};
use axum::http::HeaderMap;
use mbs4_app::state::AppState;
use mbs4_server::{
    config::{Parser, ServerConfig},
    run::{build_state, run, run_with_state},
};
use mbs4_types::claim::{ApiClaim, Role};
use rand::{Rng as _, distr::Alphanumeric};
use reqwest::{Client, Url};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt as _;
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

pub async fn random_text_file(file_path: &Path, size: u64) -> Result<()> {
    let mut file = tokio::fs::File::create(file_path).await?;

    const CHUNK_SIZE: u64 = 16 * 1024;
    let chunks = size / CHUNK_SIZE;
    let remainder = size % CHUNK_SIZE;

    let mut write_chunk = async |sz: usize| -> Result<()> {
        let random_text: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(sz)
            .map(char::from)
            .collect();
        // Write to file
        file.write_all(random_text.as_bytes()).await?;
        Ok(())
    };

    for _ in 0..chunks {
        write_chunk(CHUNK_SIZE as usize).await?;
    }
    if remainder > 0 {
        write_chunk(remainder as usize).await?;
    }
    file.flush().await?;
    Ok(())
}

pub struct ConfigGuard {
    data_dir: TempDir,
}

impl ConfigGuard {
    pub fn path(&self) -> &Path {
        self.data_dir.path()
    }
}

pub async fn prepare_env(test_name: &str) -> Result<(ServerConfig, ConfigGuard)> {
    let dir = std::env::current_dir()?;
    debug!("Current directory: {}", dir.display());
    let data_dir = dir.join("test-data");
    let (args, config_guard) = test_config(test_name, &data_dir)?;

    let pool = mbs4_dal::new_pool(&args.database_url()).await?;
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

pub async fn spawn_server_with_state(args: ServerConfig, state: AppState) -> Result<()> {
    let port = args.port;
    tokio::spawn(async move {
        println!("RUST_LOG is {}", env::var("RUST_LOG").unwrap_or_default());
        run_with_state(args, state).await.unwrap();
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

pub fn issue_token(state: &AppState, claim: ApiClaim) -> Result<String> {
    let token = state.tokens().issue(claim)?;
    Ok(token)
}

pub fn admin_token(state: &AppState) -> Result<String> {
    let claim = ApiClaim::new_expired("admin@localhost", [Role::Admin, Role::Trusted]);
    issue_token(state, claim).map_err(|e| e.into())
}

pub fn user_token(state: &AppState) -> Result<String> {
    let claim = ApiClaim::new_expired::<Role>("user@localhost", []);
    issue_token(state, claim).map_err(|e| e.into())
}

pub fn trusted_user_token(state: &AppState) -> Result<String> {
    let claim = ApiClaim::new_expired("trusted@localhost", [Role::Trusted]);
    issue_token(state, claim).map_err(|e| e.into())
}

pub fn auth_headers(token: String) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", format!("Bearer {}", token).parse()?);
    Ok(headers)
}

pub enum TestUser {
    Admin,
    TrustedUser,
    User,
    None,
}

impl TestUser {
    pub fn auth_header(&self, state: &AppState) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let mut insert = |token: String| -> Result<()> {
            headers.insert("Authorization", format!("Bearer {}", token).parse()?);

            Ok(())
        };
        match self {
            TestUser::Admin => insert(admin_token(state)?)?,
            TestUser::User => insert(user_token(state)?)?,
            TestUser::None => {}
            TestUser::TrustedUser => insert(trusted_user_token(state)?)?,
        }
        Ok(headers)
    }
}

pub async fn launch_env(args: ServerConfig, user: TestUser) -> Result<(Client, AppState)> {
    let state = build_state(&args).await?;
    let auth_headers = user.auth_header(&state)?;
    spawn_server_with_state(args, state.clone()).await?;

    let client = reqwest::Client::builder()
        .cookie_store(true)
        .default_headers(auth_headers)
        .build()?;

    Ok((client, state))
}

pub fn extend_url(api_url: &Url, segment: impl ToString) -> Url {
    let mut record_url = api_url.clone();
    record_url
        .path_segments_mut()
        .unwrap()
        .push(&segment.to_string());
    record_url
}

pub fn now() -> time::PrimitiveDateTime {
    let now = time::OffsetDateTime::now_utc();
    time::PrimitiveDateTime::new(now.date(), now.time())
}
