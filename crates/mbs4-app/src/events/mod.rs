use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
    Router,
};

use futures::{Stream, StreamExt};
use std::{convert::Infallible, time::Duration};

use crate::state::AppState;

mod hub;
pub use hub::{EventHub, SequencedEvent};

#[derive(Clone, Debug)]
pub struct EventMessage {
    pub(crate) kind: String,
    pub(crate) target_user: Option<String>,
    pub(crate) data: String,
}

impl EventMessage {
    pub fn message<T>(kind: impl ToString, target_user: Option<String>, data: T) -> Self
    where
        T: serde::Serialize,
    {
        let data = serde_json::to_string(&data).unwrap();
        Self {
            kind: kind.to_string(),
            target_user,
            data,
        }
    }
}

#[derive(serde::Deserialize)]
struct EventsQuery {
    // case-sensitive, mirrors DOM EventSource.lastEventId
    #[serde(rename = "lastEventId")]
    last_event_id: Option<String>,
}

async fn sse_handler(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
    api_user: mbs4_types::claim::ApiClaim,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Header wins: on browser auto-reconnect the query param is stale, the header is fresh
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or(query.last_event_id);
    let me = api_user.sub;
    let stream = state
        .events()
        .subscribe_after(last_event_id.as_deref())
        .filter_map(move |e| {
            let deliver = match &e.message.target_user {
                None => true,
                Some(u) => *u == me,
            };
            let item = deliver.then(|| {
                Ok(Event::default().id(&e.id).data(format!(
                    r#"{{"type":"{}","data":{}}}"#,
                    e.message.kind, e.message.data
                )))
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
