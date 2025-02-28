use axum::{response::IntoResponse, routing::get, Router};
use mbs4_app::{auth::auth_router, user::users_router};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use mbs4_server::{build_state, config::ServerConfig, Result};
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
    let state = build_state(&args).await?;

    let session_store = tower_sessions::MemoryStore::default();
    let session_layer = tower_sessions::SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(tower_sessions::Expiry::OnInactivity(
            time::Duration::seconds(15),
        ));

    let app = Router::new()
        .route("/", get(root))
        .route("/test", get(test))
        .layer(session_layer)
        .nest("/auth", auth_router())
        .nest("/users", users_router())
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

async fn root(request: axum::extract::Request) -> impl IntoResponse {
    let headers = request.headers();
    let mut headers_print = "Request headers:\n".to_string();
    for (name, value) in headers.iter() {
        headers_print.push_str(&format!("{}: {}\n", name.as_str(), value.to_str().unwrap()));
    }
    headers_print
}
