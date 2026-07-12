//! End-to-end tests for `/events` replay: on reconnect the server must
//! replay events published after the client-supplied last event id, given
//! either as the `Last-Event-ID` header or the `lastEventId` query param.

use std::time::Duration;

use mbs4_app::events::EventMessage;
use mbs4_e2e_tests::{TestUser, drain_sse_until, launch_env, prepare_env};
use reqwest::Url;
use serde_json::{Value, json};
use tracing_test::traced_test;

const TIMEOUT: Duration = Duration::from_secs(10);

fn parse_event(raw: &str) -> Option<(String, Value)> {
    let mut id = None;
    let mut data = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("id:") {
            id = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data = Some(rest.trim().to_string());
        }
    }
    let envelope: Value = serde_json::from_str(&data?).ok()?;
    Some((id?, envelope))
}

fn test_event(target_user: Option<&str>, name: &str) -> EventMessage {
    EventMessage::message(
        "test_event",
        target_user.map(str::to_string),
        json!({ "n": name }),
    )
}

/// Connects, waits for the subscription to be live, then returns the first
/// `test_event` observed after `inject` ran.
async fn first_event_after_inject(
    request: reqwest::RequestBuilder,
    inject: impl FnOnce(),
) -> (String, Value) {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let listener = tokio::spawn(async move {
        drain_sse_until(request, ready_tx, |raw| {
            let (id, envelope) = parse_event(raw)?;
            (envelope["type"] == "test_event").then_some((id, envelope))
        })
        .await
    });
    ready_rx.await.expect("SSE listener ready");
    inject();
    tokio::time::timeout(TIMEOUT, listener)
        .await
        .expect("event timeout")
        .expect("listener task")
        .expect("stream ended without event")
}

/// Collects the names of the first two `test_event`s delivered on a fresh
/// connection (replayed or live).
async fn collect_two_names(request: reqwest::RequestBuilder) -> Vec<String> {
    let (ready_tx, _ready_rx) = tokio::sync::oneshot::channel();
    let mut seen = Vec::new();
    tokio::time::timeout(
        TIMEOUT,
        drain_sse_until(request, ready_tx, |raw| {
            let (_, envelope) = parse_event(raw)?;
            if envelope["type"] != "test_event" {
                return None;
            }
            seen.push(envelope["data"]["n"].as_str().unwrap().to_string());
            (seen.len() == 2).then(|| seen.clone())
        }),
    )
    .await
    .expect("replay timeout")
    .expect("stream ended without events")
}

fn with_last_event_id(sse_url: &Url, id: &str) -> Url {
    let mut url = sse_url.clone();
    url.query_pairs_mut().append_pair("lastEventId", id);
    url
}

#[tokio::test]
#[traced_test]
async fn test_events_replay_after_reconnect() {
    let (args, mut guard) = prepare_env("test_events_replay").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();
    let sse_url = base_url.join("events").unwrap();

    // Live connection sees event A; remember its replay id.
    let (last_id, envelope) = first_event_after_inject(client.get(sse_url.clone()), || {
        state
            .events()
            .send(test_event(Some("admin@localhost"), "A"));
    })
    .await;
    assert_eq!(envelope["data"]["n"], "A");

    // While disconnected: B for admin, C for another user, D broadcast.
    state
        .events()
        .send(test_event(Some("admin@localhost"), "B"));
    state
        .events()
        .send(test_event(Some("trusted@localhost"), "C"));
    state.events().send(test_event(None, "D"));

    // Reconnect with the query param: B and D replayed in order, A not
    // re-delivered, C never visible (D doubles as the terminal marker).
    let url = with_last_event_id(&sse_url, &last_id);
    assert_eq!(collect_two_names(client.get(url)).await, ["B", "D"]);

    // Reconnect with the Last-Event-ID header; the stale query param must
    // lose to it — receiving the replay proves the header was honored.
    let url = with_last_event_id(&sse_url, "deadbeef-99");
    let request = client.get(url).header("Last-Event-ID", &last_id);
    assert_eq!(collect_two_names(request).await, ["B", "D"]);
}

#[tokio::test]
#[traced_test]
async fn test_events_unknown_last_id_starts_live() {
    let (args, mut guard) = prepare_env("test_events_unknown_id").await.unwrap();
    let base_url = args.base_url.clone();
    let (client, state) = launch_env(args, TestUser::Admin, &mut guard).await.unwrap();
    let sse_url = base_url.join("events").unwrap();

    state
        .events()
        .send(test_event(Some("admin@localhost"), "stale"));

    // Unknown id: nothing replayed, the first delivery is the live event.
    let url = with_last_event_id(&sse_url, "deadbeef-99");
    let (_, envelope) = first_event_after_inject(client.get(url), || {
        state
            .events()
            .send(test_event(Some("admin@localhost"), "live"));
    })
    .await;
    assert_eq!(envelope["data"]["n"], "live");
}
