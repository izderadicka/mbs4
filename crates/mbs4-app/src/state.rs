use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use crate::{
    ebook_format::convertor::Convertor, error::Result, events::EventMessage, search::Search,
};
use axum::extract::FromRef;
use futures::Stream;
use mbs4_auth::config::OIDCConfig;
use mbs4_auth::token::TokenManager;
use mbs4_dal::Pool;
use mbs4_store::file_store::FileStore;
use tokio_stream::StreamExt;
use tracing::{debug, error};
use url::Url;

#[derive(Clone)]
pub struct AppState {
    state: Arc<AppStateInner>,
}

impl AppState {
    pub async fn new(
        shutdown: tokio_util::sync::CancellationToken,
        oidc_config: Option<OIDCConfig>,
        app_config: AppConfig,
        pool: Pool,
        tokens: TokenManager,
        search: Search,
    ) -> anyhow::Result<Self> {
        let state = RwLock::new(AppStateVolatile {});
        let store = FileStore::new(&app_config.file_store_path);
        let events = EventHub::new();
        let convertor = Convertor::new(events.sender(), store.clone(), pool.clone()).await?;
        Ok(AppState {
            state: Arc::new(AppStateInner {
                shutdown,
                oidc_providers_config: oidc_config,
                state,
                app_config,
                pool,
                store,
                tokens,
                search,
                events,
                convertor,
            }),
        })
    }
    pub fn get_oidc_provider(&self, name: &str) -> Option<mbs4_auth::config::OIDCProviderConfig> {
        self.state
            .oidc_providers_config
            .as_ref()
            .and_then(|c| c.get_provider(name).cloned())
    }

    pub fn known_oidc_providers(&self) -> Vec<String> {
        self.state
            .oidc_providers_config
            .as_ref()
            .map(|c| c.available_providers())
            .unwrap_or_default()
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

    pub fn events(&self) -> &EventHub {
        &self.state.events
    }

    pub fn convertor(&self) -> &Convertor {
        &self.state.convertor
    }

    pub fn shutdown_signal(&self) -> &tokio_util::sync::CancellationToken {
        &self.state.shutdown
    }
}

pub struct EventHub {
    sender: tokio::sync::broadcast::Sender<EventMessage>,
}

impl EventHub {
    pub fn new() -> Self {
        let (sender, mut receiver) = tokio::sync::broadcast::channel(1024);
        #[cfg(debug_assertions)]
        tokio::spawn(async move {
            while let Ok(msg) = receiver.recv().await {
                debug!("Event: {msg:?}");
            }
        });
        EventHub { sender }
    }

    pub fn send(&self, msg: EventMessage) {
        if let Err(e) = self.sender.send(msg) {
            debug!("Nowhere send event: {e}");
        };
    }

    pub fn sender(&self) -> tokio::sync::broadcast::Sender<EventMessage> {
        self.sender.clone()
    }

    pub fn receiver(&self) -> tokio::sync::broadcast::Receiver<EventMessage> {
        self.sender.subscribe()
    }

    pub fn receiver_stream(&self) -> impl Stream<Item = EventMessage> {
        tokio_stream::wrappers::BroadcastStream::new(self.receiver()).filter_map(|r| {
            r.inspect_err(|e| error!("EventHub receiver lags: {e}"))
                .ok()
        })
    }
}

struct AppStateInner {
    pool: Pool,
    oidc_providers_config: Option<OIDCConfig>,
    app_config: AppConfig,
    tokens: TokenManager,
    store: FileStore,
    #[allow(dead_code)]
    state: RwLock<AppStateVolatile>,
    search: Search,
    events: EventHub,
    convertor: Convertor,
    shutdown: tokio_util::sync::CancellationToken,
}

#[derive(Debug)]
pub struct AppConfig {
    pub base_url: Url,
    pub base_backend_url: Url,
    pub file_store_path: PathBuf,
    pub static_dir: Option<PathBuf>,
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
