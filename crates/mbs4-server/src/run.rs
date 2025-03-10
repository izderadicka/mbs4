use std::path::Path;

use crate::config::ServerConfig;
use crate::error::Result;
use axum::{response::IntoResponse, routing::get, Router};
use mbs4_app::state::{AppConfig, AppState};
use mbs4_app::{
    auth::{
        auth_router,
        token::{RequiredRolesLayer, TokenLayer},
    },
    user::users_router,
};
use mbs4_types::claim::ApiClaim;
use mbs4_types::oidc::OIDCConfig;
use tokio::{fs, io::AsyncWriteExt as _, task::spawn_blocking};
use tower::ServiceBuilder;
use tracing::debug;

pub async fn run(args: ServerConfig) -> Result<()> {
    let state = build_state(&args).await?;

    let app = main_router(state);

    let ip: std::net::IpAddr = args.listen_address.parse()?;
    let addr = std::net::SocketAddr::from((ip, args.port));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    debug!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await?;
    Ok(())
}

fn main_router(state: AppState) -> Router<()> {
    // Not needed now
    // let session_store = tower_sessions::MemoryStore::default();
    // let session_layer = tower_sessions::SessionManagerLayer::new(session_store)
    //     .with_secure(false)
    //     .with_expiry(tower_sessions::Expiry::OnInactivity(
    //         time::Duration::seconds(15),
    //     ));

    Router::new()
        .route("/", get(root))
        // .layer(session_layer)
        .nest("/auth", auth_router())
        .nest("/users", users_router())
        .route(
            "/protected",
            get(protected).layer(
                ServiceBuilder::new()
                    .layer(TokenLayer::new(state.clone()))
                    .layer(RequiredRolesLayer::new(["admin"])),
            ),
        )
        .layer(tower_cookies::CookieManagerLayer::new())
        .with_state(state)
}

async fn protected(claim: ApiClaim) -> impl IntoResponse {
    format!("This is a protected route, welcome {claim:?}")
}

async fn root(request: axum::extract::Request) -> impl IntoResponse {
    let headers = request.headers();
    let mut headers_print = "Request headers:\n".to_string();
    for (name, value) in headers.iter() {
        headers_print.push_str(&format!("{}: {}\n", name.as_str(), value.to_str().unwrap()));
    }
    headers_print
}

pub async fn build_state(config: &ServerConfig) -> Result<AppState> {
    let oidc_config_file = config.oidc_config.clone();
    let oidc_config = spawn_blocking(move || OIDCConfig::load_config(&oidc_config_file)).await??;

    let app_config = AppConfig {
        base_url: config.base_url.clone(),
    };

    let pool = mbs4_dal::new_pool(&config.database_url).await?;
    // Its OK here to block, as it's short and called only on init;
    let data_dir = config.data_dir()?;
    let secret = read_secret(&data_dir).await?;
    let tokens = mbs4_auth::token::TokenManager::new(&secret, config.token_validity);
    Ok(AppState::new(oidc_config, app_config, pool, tokens))
}

async fn read_secret(data_dir: &Path) -> Result<Vec<u8>, std::io::Error> {
    let secret_file = data_dir.join("secret");

    let secret = if fs::try_exists(&secret_file).await? {
        fs::read(&secret_file).await?
    } else {
        let random_bytes = rand::random::<[u8; 32]>();
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
