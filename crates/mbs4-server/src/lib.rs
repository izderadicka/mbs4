pub mod config;
pub mod error;

use config::ServerConfig;
pub use error::{Error, Result};
use mbs4_app::state::{AppConfig, AppState};
use mbs4_types::oidc::OIDCConfig;
use sqlx::sqlite::SqlitePoolOptions;
use tokio::{fs, io::AsyncWriteExt as _, task::spawn_blocking};

pub async fn build_state(config: &ServerConfig) -> Result<AppState> {
    let oidc_config_file = config.oidc_config.clone();
    let oidc_config = spawn_blocking(move || OIDCConfig::load_config(&oidc_config_file)).await??;

    let app_config = AppConfig {
        base_url: config.base_url.clone(),
    };

    let pool = SqlitePoolOptions::new()
        .max_connections(50)
        .connect(&config.database_url)
        .await?;
    // Its OK here to block, as it's short and called only on init;
    let data_dir = config.data_dir()?;
    let secret_file = data_dir.join("secret");
    let secret = if fs::try_exists(&secret_file).await? {
        fs::read(&secret_file).await?
    } else {
        let random_bytes = rand::random::<[u8; 32]>();
        fs::File::create(&secret_file)
            .await?
            .write_all(&random_bytes)
            .await?;
        random_bytes.as_ref().to_vec()
    };
    let tokens = mbs4_auth::token::TokenManager::new(&secret, config.token_validity);
    Ok(AppState::new(oidc_config, app_config, pool, tokens))
}
