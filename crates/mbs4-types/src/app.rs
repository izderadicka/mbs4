use std::sync::{Arc, RwLock};

use crate::config::ServerConfig;
use crate::error::Result;
use crate::oidc::OIDCConfig;
use tokio::task::spawn_blocking;

#[derive(Clone)]
pub struct AppState {
    state: Arc<AppStateInner>,
}

impl AppState {
    pub fn get_oidc_provider(&self, name: &str) -> Option<crate::oidc::OIDCProviderConfig> {
        self.state.oidc_providers_config.get_provider(name).cloned()
    }

    pub fn get_app_config(&self) -> &AppConfig {
        &self.state.app_config
    }

    pub async fn build(config: &ServerConfig) -> Result<Self> {
        let oidc_config_file = config.oidc_config.clone();
        let oidc_config =
            spawn_blocking(move || OIDCConfig::load_config(&oidc_config_file)).await??;
        let state = RwLock::new(AppStateVolatile {});
        let app_config = AppConfig {
            base_url: config.base_url.clone(),
        };
        Ok(AppState {
            state: Arc::new(AppStateInner {
                oidc_providers_config: oidc_config,
                state,
                app_config,
            }),
        })
    }
}

pub struct AppStateInner {
    oidc_providers_config: OIDCConfig,
    app_config: AppConfig,
    #[allow(dead_code)]
    state: RwLock<AppStateVolatile>,
}

pub struct AppConfig {
    pub base_url: String,
}

pub struct AppStateVolatile {}
