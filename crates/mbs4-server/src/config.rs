use core::panic;
use std::{fs, path::PathBuf, time::Duration};

use crate::error::Result;
pub use clap::Parser;
use url::Url;

#[derive(Debug, Clone, clap::Parser)]
pub struct ServerConfig {
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
        help = "Path to OIDC configuration file, default is oidc-config.toml in data directory"
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
        default_value = "http://localhost:3000",
        help = "Base URL of server, if different from base_url, defaults to base_url"
    )]
    pub base_backend_url: Option<Url>,

    #[arg(
        long,
        env = "MBS4_DATABASE_URL",
        help = "Database URL e.g. sqlite://file.db or similar, default is sqlite://[data-dir]/mbs4.db, where data-dir is set by --data-dir"
    )]
    database_url: Option<String>,

    #[arg(
        long,
        env = "MBS4_INDEX_PATH",
        help = "Path to fulltext search index, default is [data-dir]/mbs4-ft-idx.db, where data-dir is set by --data-dir"
    )]
    index_path: Option<PathBuf>,

    #[arg(
        long,
        env = "MBS4_DATA_DIR",
        help = "Data directory (ebook files, databases, configs etc.), default is system default like ~/.local/share/mbs4",
        default_value_t = default_data_dir()
    )]
    data_dir: String,

    #[arg(
        long,
        env = "MBS4_FILES_DIR",
        help = "Directory for book files, default data_dir/ebooks"
    )]
    files_dir: Option<PathBuf>,

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

    #[arg(long, env = "MBS4_NO_CORS", help = "Disable CORS")]
    pub no_cors: bool,
}

fn default_data_dir() -> String {
    let dir = dirs::data_dir()
        .map(|p| p.join("mbs4"))
        .unwrap_or_else(|| PathBuf::from("mbs4"));

    if !fs::exists(&dir).expect("Failed to check if data directory exists") {
        fs::create_dir_all(&dir).expect("Failed to create data directory");
    } else if !dir.is_dir() {
        panic!("Data directory is not a directory",)
    }

    dir.to_string_lossy().to_string()
}

impl ServerConfig {
    pub fn load() -> Result<Self> {
        ServerConfig::try_parse().map_err(|e| e.into())
    }

    pub fn data_dir(&self) -> PathBuf {
        PathBuf::from(&self.data_dir)
    }

    pub fn files_dir(&self) -> PathBuf {
        self.files_dir
            .clone()
            .unwrap_or_else(|| self.data_dir().join("ebooks"))
    }

    pub fn database_url(&self) -> String {
        self.database_url
            .clone()
            .unwrap_or_else(|| format!("sqlite://{}/mbs4.db", self.data_dir))
    }

    pub fn index_path(&self) -> PathBuf {
        self.index_path
            .clone()
            .unwrap_or_else(|| self.data_dir().join("mbs4-ft-idx.db"))
    }
}
