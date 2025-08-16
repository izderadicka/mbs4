use axum::{
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
    Router,
};

use futures::Stream;
use std::{convert::Infallible, time::Duration};
use tokio_stream::StreamExt as _;

use crate::state::AppState;

async fn sse_handler() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Example: a stream of 10 events with 1-second intervals
    let stream = tokio_stream::iter(0..10)
        .throttle(Duration::from_secs(1))
        .map(|i| {
            Ok(Event::default().data(format!("tick {i}")).event("message")) // optional event name
        });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive-text"),
    )
}

pub fn events_router() -> Router<AppState> {
    Router::new().route("/", get(sse_handler))
}
