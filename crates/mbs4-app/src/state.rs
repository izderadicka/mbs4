use std::sync::{Arc, RwLock};

use crate::error::Result;
use mbs4_auth::token::TokenManager;
use mbs4_dal::Pool;
use mbs4_types::oidc::OIDCConfig;
use url::Url;

#[derive(Clone)]
pub struct AppState {
    state: Arc<AppStateInner>,
}

impl AppState {
    pub fn new(
        oidc_config: OIDCConfig,
        app_config: AppConfig,
        pool: Pool,
        tokens: TokenManager,
    ) -> Self {
        let state = RwLock::new(AppStateVolatile {});
        AppState {
            state: Arc::new(AppStateInner {
                oidc_providers_config: oidc_config,
                state,
                app_config,
                pool,
                tokens,
            }),
        }
    }
    pub fn get_oidc_provider(&self, name: &str) -> Option<mbs4_types::oidc::OIDCProviderConfig> {
        self.state.oidc_providers_config.get_provider(name).cloned()
    }

    pub fn get_app_config(&self) -> &AppConfig {
        &self.state.app_config
    }

    pub fn build_url(&self, relative_url: &str) -> Result<Url> {
        let base = &self.get_app_config().base_url;
        let url = base.join(relative_url)?;
        Ok(url)
    }

    pub fn pool(&self) -> &Pool {
        &self.state.pool
    }

    pub fn tokens(&self) -> &TokenManager {
        &self.state.tokens
    }
}

struct AppStateInner {
    pool: Pool,
    oidc_providers_config: OIDCConfig,
    app_config: AppConfig,
    tokens: TokenManager,
    #[allow(dead_code)]
    state: RwLock<AppStateVolatile>,
}

pub struct AppConfig {
    pub base_url: Url,
}

pub struct AppStateVolatile {}
