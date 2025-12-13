use clap::Parser;
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Parser)]
pub struct BackendConfig {
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

impl BackendConfig {
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
