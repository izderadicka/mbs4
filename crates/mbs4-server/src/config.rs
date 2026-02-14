use std::{path::PathBuf, time::Duration};

use crate::error::Result;
pub use clap::Parser;
use mbs4_app::state::AppConfig;
use mbs4_types::config::BackendConfig;
use url::Url;

#[derive(Debug, Clone, clap::Parser)]
pub struct ServerConfig {
    #[command(flatten)]
    backend: BackendConfig,
    #[arg(
        short,
        long,
        default_value_t = 3000,
        env = "MBS4_LISTEN_PORT",
        help = "Port to listen on"
    )]
    pub port: u16,
    #[arg(
        short,
        long,
        default_value = "127.0.0.1",
        env = "MBS4_LISTEN_ADDRESS",
        help = "Address to listen on"
    )]
    pub listen_address: String,

    #[arg(
        long,
        env = "MBS4_OIDC_CONFIG",
        help = "Path to OIDC configuration file, default location is in data directory"
    )]
    pub oidc_config: Option<String>,

    #[arg(
        long,
        env = "MBS4_BASE_URL",
        default_value = "http://localhost:3000",
        help = "Base URL of frontend app and server, as visible to users"
    )]
    pub base_url: Url,

    #[arg(
        long,
        env = "MBS4_BASE_URL",
        help = "Base URL of server, if different from base_url, defaults to base_url"
    )]
    pub base_backend_url: Option<Url>,

    #[arg(
        long,
        env = "MBS4_TOKEN_VALIDITY",
        default_value = "1 day",
        help = "Default token validity in human friendtly format (e.g. 1d, 1h, 1m, 1s - or combined)",
        value_parser = humantime::parse_duration
    )]
    pub token_validity: Duration,

    #[arg(
        long,
        env = "MBS4_UPLOAD_LIMIT_MB",
        default_value = "100",
        help = "Maximum upload size in MB"
    )]
    pub upload_limit_mb: usize,

    #[arg(
        long,
        env = "MBS4_DEFAULT_PAGE_SIZE",
        default_value = "100",
        help = "Default page size"
    )]
    pub default_page_size: u32,

    #[arg(
        long,
        env = "MBS4_CORS",
        help = "Enable CORS and also Cookies SameSite=None, useful for development, production should be false and web clients should have same URL as backend",
        default_value = "false"
    )]
    pub cors: bool,

    #[arg(
        long,
        env = "MBS4_STATIC_DIR",
        default_value = "static",
        help = "Path to static client files, if provided will be served by server"
    )]
    pub static_dir: Option<PathBuf>,
}

impl ServerConfig {
    pub fn load() -> Result<Self> {
        ServerConfig::try_parse().map_err(|e| e.into())
    }

    pub fn data_dir(&self) -> PathBuf {
        self.backend.data_dir()
    }

    pub fn files_dir(&self) -> PathBuf {
        self.backend.files_dir()
    }

    pub fn database_url(&self) -> String {
        self.backend.database_url()
    }

    pub fn index_path(&self) -> PathBuf {
        self.backend.index_path()
    }
}

impl From<&ServerConfig> for AppConfig {
    fn from(config: &ServerConfig) -> Self {
        AppConfig {
            base_url: config.base_url.clone(),
            base_backend_url: config
                .base_backend_url
                .as_ref()
                .cloned()
                .unwrap_or_else(|| config.base_url.clone()),
            file_store_path: config.files_dir(),
            upload_limit_mb: config.upload_limit_mb,
            default_page_size: config.default_page_size,
            cors: config.cors,
            static_dir: config.static_dir.clone(),
        }
    }
}
