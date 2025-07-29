use std::path::Path;

use crate::config::ServerConfig;
use crate::error::Result;
use axum::http::StatusCode;
use axum::{response::IntoResponse, routing::get, Router};
use futures::FutureExt;
use mbs4_app::search::Search;
use mbs4_app::state::{AppConfig, AppState};
use mbs4_app::store::store_router;
use mbs4_app::{
    auth::{auth_router, token::TokenLayer},
    user::users_router,
};
use mbs4_types::oidc::OIDCConfig;
use tokio::{fs, io::AsyncWriteExt as _, task::spawn_blocking};
use tracing::{debug, info};

pub async fn run(args: ServerConfig) -> Result<()> {
    let state = build_state(&args).await?;
    run_with_state(args, state).await
}

pub async fn run_with_state(args: ServerConfig, state: AppState) -> Result<()> {
    let shutdown = tokio::signal::ctrl_c().map(|_| ());
    run_graceful_with_state(args, state, shutdown).await
}

pub async fn run_graceful_with_state<S>(
    args: ServerConfig,
    state: AppState,
    shutdown_signal: S,
) -> Result<()>
where
    S: std::future::Future<Output = ()> + Send + 'static,
{
    let mut app = main_router(state);

    if args.cors {
        app = app.layer(
            // TODO: Consider if we want to allow credentials and restrict headers
            tower_http::cors::CorsLayer::very_permissive(),
        );
    }

    let ip: std::net::IpAddr = args.listen_address.parse()?;
    let addr = std::net::SocketAddr::from((ip, args.port));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    debug!("Listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    Ok(())
}

#[cfg(feature = "openapi")]
fn api_docs() -> utoipa::openapi::OpenApi {
    use utoipa::openapi::Components;

    #[derive(utoipa::OpenApi)]
    #[openapi(modifiers(&SecurityAddon), security(("bearer" = [])))]
    struct OpenApi;

    struct SecurityAddon;

    impl utoipa::Modify for SecurityAddon {
        fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
            use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

            if openapi.components.is_none() {
                openapi.components = Some(Components::new());
            }

            openapi.components.as_mut().unwrap().add_security_scheme(
                "bearer",
                SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
            );
        }
    }

    use utoipa::OpenApi as _;
    OpenApi::openapi()
        .nest("/api/ebook", mbs4_app::rest_api::ebook::api_docs())
        .nest("/api/format", mbs4_app::rest_api::format::api_docs())
        .nest("/api/genre", mbs4_app::rest_api::genre::api_docs())
        .nest("/api/language", mbs4_app::rest_api::language::api_docs())
        .nest("/api/series", mbs4_app::rest_api::series::api_docs())
        .nest("/api/source", mbs4_app::rest_api::source::api_docs())
        .nest("/api/author", mbs4_app::rest_api::author::api_docs())
        .nest("/auth", mbs4_app::auth::api_docs())
}

fn main_router(state: AppState) -> Router<()> {
    // Not needed now
    // let session_store = tower_sessions::MemoryStore::default();
    // let session_layer = tower_sessions::SessionManagerLayer::new(session_store)
    //     .with_secure(false)
    //     .with_expiry(tower_sessions::Expiry::OnInactivity(
    //         time::Duration::seconds(15),
    //     ));

    #[allow(unused_mut)]
    let mut router = Router::new()
        .nest("/users", users_router())
        .nest("/store", store_router(state.config().upload_limit_mb))
        .nest("/api/language", mbs4_app::rest_api::language::router())
        .nest("/api/format", mbs4_app::rest_api::format::router())
        .nest("/api/genre", mbs4_app::rest_api::genre::router())
        .nest("/api/series", mbs4_app::rest_api::series::router())
        .nest("/api/source", mbs4_app::rest_api::source::router())
        .nest("/api/author", mbs4_app::rest_api::author::router())
        .nest("/api/ebook", mbs4_app::rest_api::ebook::router())
        .nest("/search", mbs4_app::search::router())
        // All above routes are protected
        .layer(TokenLayer::new(state.clone()))
        .nest("/auth", auth_router())
        .layer(tower_cookies::CookieManagerLayer::new())
        .with_state(state)
        // static and public resources
        .route("/", get(root))
        .route("/index.html", get(root)) // TODO - change
        .route("/health", get(health));

    #[cfg(feature = "openapi")]
    {
        let docs = api_docs();
        router = router.merge(
            utoipa_swagger_ui::SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", docs),
        );
    }
    router
}

async fn root(request: axum::extract::Request) -> impl IntoResponse {
    let headers = request.headers();
    let mut headers_print = "Request headers:\n".to_string();
    for (name, value) in headers.iter() {
        headers_print.push_str(&format!("{}: {}\n", name.as_str(), value.to_str().unwrap()));
    }
    headers_print
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

pub async fn build_state(config: &ServerConfig) -> Result<AppState> {
    let data_dir = config.data_dir();
    let oidc_config_file = config.oidc_config.clone().unwrap_or_else(|| {
        let path = data_dir.join("oidc-config.toml");
        path.to_string_lossy().to_string()
    });
    let oidc_config = spawn_blocking(move || OIDCConfig::load_config(&oidc_config_file)).await??;

    let app_config: AppConfig = config.into();

    if !app_config.file_store_path.is_dir() {
        tokio::fs::create_dir_all(&app_config.file_store_path).await?;
        info!("Created directory for ebook files");
    }

    let pool = mbs4_dal::new_pool(&config.database_url()).await?;

    // Its OK here to block, as it's short and called only on init;

    let secret = read_secret(&data_dir).await?;
    assert!(secret.len() == 64);
    let tokens =
        mbs4_auth::token::TokenManager::new(&secret[0..32], &secret[32..], config.token_validity);
    let search = Search::new(&config.index_path(), pool.clone()).await?;
    Ok(AppState::new(oidc_config, app_config, pool, tokens, search))
}

async fn read_secret(data_dir: &Path) -> Result<Vec<u8>, std::io::Error> {
    let secret_file = data_dir.join("secret");

    let secret = if fs::try_exists(&secret_file).await? {
        fs::read(&secret_file).await?
    } else {
        let random_bytes = rand::random::<[u8; 64]>();
        #[cfg(unix)]
        let mut file = {
            use std::fs::OpenOptions;
            use std::os::unix::fs::OpenOptionsExt;
            {
                // Make sure the file is only accessible by the current user
                let _f = OpenOptions::new()
                    .mode(0o600)
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&secret_file)?;
            }
            fs::File::options().write(true).open(&secret_file).await?
        };
        #[cfg(not(unix))]
        let mut file = fs::File::create(&secret_file).await?;

        file.write_all(&random_bytes).await?;
        random_bytes.as_ref().to_vec()
    };
    Ok(secret)
}
