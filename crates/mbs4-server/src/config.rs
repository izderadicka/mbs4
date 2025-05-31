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
        help = "Base URL of server, as visible to users"
    )]
    pub base_url: Url,

    #[arg(
        long,
        default_value = "sqlite://test-data/mbs4.db",
        env = "MBS4_DATABASE_URL",
        help = "Database URL e.g. sqlite://file.db or similar"
    )]
    pub database_url: String,

    #[arg(
        long,
        default_value = "test-data/mbs4-ft-idx.db",
        env = "MBS4_INDEX_PATH",
        help = "Path to fulltext search index"
    )]
    pub index_path: PathBuf,

    #[arg(
        long,
        env = "MBS4_DATA_DIR",
        help = "Data directory, default is system default like ~/.local/share/mbs4"
    )]
    pub data_dir: Option<PathBuf>,

    #[arg(
        long,
        env = "MBS4_FILES_DIR",
        help = "Directory for book files, default data_dir/ebooks"
    )]
    pub files_dir: Option<PathBuf>,

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
}

impl ServerConfig {
    pub fn load() -> Result<Self> {
        ServerConfig::try_parse().map_err(|e| e.into())
    }

    pub fn data_dir(&self) -> Result<PathBuf, std::io::Error> {
        if let Some(data_dir) = &self.data_dir {
            return Ok(data_dir.clone());
        } else {
            let dir = dirs::data_dir()
                .map(|p| p.join("mbs4"))
                .unwrap_or_else(|| PathBuf::from("mbs4"));

            if !fs::exists(&dir)? {
                fs::create_dir_all(&dir)?;
                Ok(dir)
            } else if dir.is_dir() {
                Ok(dir)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Data directory is not a directory",
                ))
            }
        }
    }
}
