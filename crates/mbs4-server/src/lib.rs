pub mod config;
pub mod error;

use config::ServerConfig;
pub use error::{Error, Result};
use mbs4_app::state::{AppConfig, AppState};
use mbs4_types::oidc::OIDCConfig;
use tokio::task::spawn_blocking;

pub async fn build_state(config: &ServerConfig) -> Result<AppState> {
    let oidc_config_file = config.oidc_config.clone();
    let oidc_config = spawn_blocking(move || OIDCConfig::load_config(&oidc_config_file)).await??;

    let app_config = AppConfig {
        base_url: config.base_url.clone(),
    };
    Ok(AppState::new(oidc_config, app_config))
}
