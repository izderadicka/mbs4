use axum::{response::IntoResponse, routing::get, Router};
use mbs4_auth::web::auth_router;
use mbs4_types::{app::AppState, config::ServerConfig};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use mbs4_server::Result;
use tracing::debug;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct Counter {
    count: u32,
    timestamp: u64,
}

const COUNTER_KEY: &str = "mbs3_counter";

impl Counter {
    pub fn increment(&mut self) {
        self.count += 1;
        self.timestamp = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = ServerConfig::load()?;
    let state = AppState::build(&args).await?;

    let session_store = tower_sessions::MemoryStore::default();
    let session_layer = tower_sessions::SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(tower_sessions::Expiry::OnInactivity(
            time::Duration::seconds(15),
        ));

    let app = Router::new()
        .route("/test", get(test))
        .layer(session_layer)
        .nest("/auth", auth_router())
        .with_state(state);
    let ip: std::net::IpAddr = args.listen_address.parse()?;
    let addr = std::net::SocketAddr::from((ip, args.port));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    debug!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await?;
    Ok(())
}

async fn test(session: Session, _request: axum::extract::Request) -> impl IntoResponse {
    let mut counter: Counter = session
        .get(COUNTER_KEY)
        .await
        .unwrap()
        .unwrap_or(Counter::default());
    counter.increment();
    let text = format!("test count {} at {}", &counter.count, counter.timestamp);
    session.insert(COUNTER_KEY, counter).await.unwrap();
    text
}
