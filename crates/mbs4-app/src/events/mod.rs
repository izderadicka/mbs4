use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
    Router,
};

use futures::{Stream, StreamExt};
use std::{convert::Infallible, fmt::Display, time::Duration};

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
    target_user: Option<String>,
    data: String,
}

impl EventMessage {
    pub fn new<T>(id: impl ToString, kind: EventType, target_user: Option<String>, data: T) -> Self
    where
        T: serde::Serialize,
    {
        let data = serde_json::to_string(&data).unwrap();
        Self {
            id: id.to_string(),
            kind,
            target_user,
            data,
        }
    }

    pub fn message<T>(id: impl ToString, target_user: Option<String>, data: T) -> Self
    where
        T: serde::Serialize,
    {
        Self::new(id, EventType::Message, target_user, data)
    }
}

async fn sse_handler(
    State(state): State<AppState>,
    api_user: mbs4_types::claim::ApiClaim,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let me = api_user.sub;
    let stream = state.events().receiver_stream().filter_map(move |e| {
        let deliver = match &e.target_user {
            None => true,
            Some(u) => *u == me,
        };
        let item = deliver.then(|| {
            Ok(Event::default()
                .id(e.id)
                .data(format!(r#"{{"type":"{}","data":{} }}"#, e.kind, e.data)))
        });
        std::future::ready(item)
    });

    let cancelable_stream = stream.take_until(state.shutdown_signal().clone().cancelled_owned());

    Sse::new(cancelable_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("ping"),
    )
}

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(sse_handler))
}
