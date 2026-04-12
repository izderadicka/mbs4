use std::{
    collections::HashMap,
    env,
    net::TcpListener,
    path::Path,
    sync::{Mutex, OnceLock},
};

use anyhow::{Result, anyhow};
use axum::http::HeaderMap;
use mbs4_app::state::AppState;
use mbs4_server::{
    config::{Parser, ServerConfig},
    run::{build_state, run_with_state_and_listener},
};
use mbs4_types::claim::{ApiClaim, Role};
use rand::{Rng as _, distr::Alphanumeric};
use reqwest::{Client, Url};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt as _;
use tracing::debug;

pub mod rest;

fn reserved_listeners() -> &'static Mutex<HashMap<u16, TcpListener>> {
    static RESERVED: OnceLock<Mutex<HashMap<u16, TcpListener>>> = OnceLock::new();
    RESERVED.get_or_init(|| Mutex::new(HashMap::new()))
}

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

fn reserve_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    reserved_listeners().lock().unwrap().insert(port, listener);
    Ok(port)
}

fn take_reserved_listener(port: u16) -> Result<tokio::net::TcpListener> {
    let listener = reserved_listeners()
        .lock()
        .unwrap()
        .remove(&port)
        .ok_or_else(|| anyhow!("Reserved listener for port {port} not found"))?;
    listener.set_nonblocking(true)?;
    Ok(tokio::net::TcpListener::from_std(listener)?)
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
    server: Option<TestServerHandle>,
}

struct TestServerHandle {
    shutdown: tokio_util::sync::CancellationToken,
    task: tokio::task::JoinHandle<Result<()>>,
}

impl ConfigGuard {
    pub fn path(&self) -> &Path {
        self.data_dir.path()
    }

    fn set_server(&mut self, server: TestServerHandle) {
        self.server = Some(server);
    }
}

impl Drop for ConfigGuard {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.shutdown.cancel();
            tokio::spawn(async move {
                let mut task = server.task;
                tokio::select! {
                    _ = &mut task => {}
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                        task.abort();
                    }
                }
            });
        }
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

pub async fn spawn_server(args: ServerConfig, guard: &mut ConfigGuard) -> Result<()> {
    let state = build_state(&args).await?;
    spawn_server_with_state(args, state, guard).await
}

pub async fn spawn_server_with_state(
    args: ServerConfig,
    state: AppState,
    guard: &mut ConfigGuard,
) -> Result<()> {
    let port = args.port;
    let listener = take_reserved_listener(port)?;
    let shutdown = state.shutdown_signal().clone();
    let task = tokio::spawn(async move {
        println!("RUST_LOG is {}", env::var("RUST_LOG").unwrap_or_default());
        run_with_state_and_listener(args, state, listener).await
    });

    test_port(port).await?;
    guard.set_server(TestServerHandle { shutdown, task });
    Ok(())
}

pub fn test_config(test_name: &str, base_dir: &Path) -> Result<(ServerConfig, ConfigGuard)> {
    let tmp_data_dir = TempDir::with_prefix_in(format!("{}_", test_name), base_dir)?;
    let data_dir = tmp_data_dir.path().to_string_lossy().to_string();
    let port = reserve_port()?;
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
            server: None,
        },
    ))
}

pub fn issue_token(state: &AppState, claim: ApiClaim) -> Result<String> {
    let token = state.tokens().issue(claim)?;
    Ok(token)
}

pub fn admin_token(state: &AppState) -> Result<String> {
    let claim = ApiClaim::new_expired("admin@localhost", [Role::Admin, Role::Trusted]);
    issue_token(state, claim)
}

pub fn user_token(state: &AppState) -> Result<String> {
    let claim = ApiClaim::new_expired::<Role>("user@localhost", []);
    issue_token(state, claim)
}

pub fn trusted_user_token(state: &AppState) -> Result<String> {
    let claim = ApiClaim::new_expired("trusted@localhost", [Role::Trusted]);
    issue_token(state, claim)
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

pub async fn launch_env(
    args: ServerConfig,
    user: TestUser,
    guard: &mut ConfigGuard,
) -> Result<(Client, AppState)> {
    let state = build_state(&args).await?;
    let auth_headers = user.auth_header(&state)?;
    spawn_server_with_state(args, state.clone(), guard).await?;

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
