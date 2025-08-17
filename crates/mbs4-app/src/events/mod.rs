use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
    Router,
};

use futures::Stream;
use std::{convert::Infallible, fmt::Display, time::Duration};
use tokio_stream::StreamExt as _;

use crate::state::AppState;

#[derive(Clone, Debug)]
pub enum EventType {
    Message,
}

impl Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::Message => write!(f, "message"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct EventMessage {
    id: String,
    kind: EventType,
    data: String,
}

impl EventMessage {
    pub fn new<T>(id: impl ToString, kind: EventType, data: T) -> Self
    where
        T: serde::Serialize,
    {
        let data = serde_json::to_string(&data).unwrap();
        Self {
            id: id.to_string(),
            kind,
            data,
        }
    }

    pub fn message<T>(id: impl ToString, data: T) -> Self
    where
        T: serde::Serialize,
    {
        Self::new(id, EventType::Message, data)
    }
}

async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = state.events().receiver_stream().map(|e| {
        Ok(Event::default()
            .id(e.id)
            .data(format!(r#"{{"type":"{}","data":{} }}"#, e.kind, e.data)))
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("ping"),
    )
}

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(sse_handler))
}
