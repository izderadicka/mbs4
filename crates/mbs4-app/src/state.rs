use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use crate::{error::Result, search::Search};
use axum::extract::FromRef;
use mbs4_auth::token::TokenManager;
use mbs4_dal::Pool;
use mbs4_store::file_store::FileStore;
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
        search: Search,
    ) -> Self {
        let state = RwLock::new(AppStateVolatile {});
        let store = FileStore::new(&app_config.file_store_path);
        AppState {
            state: Arc::new(AppStateInner {
                oidc_providers_config: oidc_config,
                state,
                app_config,
                pool,
                store,
                tokens,
                search,
            }),
        }
    }
    pub fn get_oidc_provider(&self, name: &str) -> Option<mbs4_types::oidc::OIDCProviderConfig> {
        self.state.oidc_providers_config.get_provider(name).cloned()
    }

    pub fn config(&self) -> &AppConfig {
        &self.state.app_config
    }

    pub fn build_url(&self, relative_url: &str) -> Result<Url> {
        let base = &self.config().base_url;
        let url = base.join(relative_url)?;
        Ok(url)
    }

    pub fn build_backend_url(&self, relative_url: &str) -> Result<Url> {
        let base = &self.config().base_backend_url;
        let url = base.join(relative_url)?;
        Ok(url)
    }

    pub fn pool(&self) -> &Pool {
        &self.state.pool
    }

    pub fn store(&self) -> &FileStore {
        &self.state.store
    }

    pub fn tokens(&self) -> &TokenManager {
        &self.state.tokens
    }

    pub fn search(&self) -> &Search {
        &self.state.search
    }
}

struct AppStateInner {
    pool: Pool,
    oidc_providers_config: OIDCConfig,
    app_config: AppConfig,
    tokens: TokenManager,
    store: FileStore,
    #[allow(dead_code)]
    state: RwLock<AppStateVolatile>,
    search: Search,
}

pub struct AppConfig {
    pub base_url: Url,
    pub base_backend_url: Url,
    pub file_store_path: PathBuf,
    pub upload_limit_mb: usize,
    pub default_page_size: u32,
    pub cors: bool,
}

pub struct AppStateVolatile {}

impl FromRef<AppState> for () {
    fn from_ref(_input: &AppState) -> Self {
        ()
    }
}
