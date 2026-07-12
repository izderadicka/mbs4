use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures::Stream;
use tokio_stream::StreamExt;
use tracing::{debug, error};

use super::EventMessage;

const RETENTION_MAX_EVENTS: usize = 1024;
const RETENTION_MAX_AGE: Duration = Duration::from_secs(300);

#[derive(Debug)]
pub struct SequencedEvent {
    pub seq: u64,
    pub id: String,
    pub created: Instant,
    pub message: EventMessage,
}

#[derive(Clone)]
pub struct EventHub {
    inner: Arc<EventHubInner>,
}

struct EventHubInner {
    boot_id: String,
    sender: tokio::sync::broadcast::Sender<Arc<SequencedEvent>>,
    buffer: Mutex<EventBuffer>,
    max_events: usize,
    max_age: Duration,
}

struct EventBuffer {
    next_seq: u64,
    events: VecDeque<Arc<SequencedEvent>>,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    pub fn new() -> Self {
        Self::with_retention(RETENTION_MAX_EVENTS, RETENTION_MAX_AGE)
    }

    pub(crate) fn with_retention(max_events: usize, max_age: Duration) -> Self {
        let (sender, mut _receiver) = tokio::sync::broadcast::channel(1024);
        #[cfg(debug_assertions)]
        tokio::spawn(async move {
            while let Ok(msg) = _receiver.recv().await {
                debug!("Event: {msg:?}");
            }
        });
        EventHub {
            inner: Arc::new(EventHubInner {
                boot_id: uuid::Uuid::new_v4().simple().to_string(),
                sender,
                buffer: Mutex::new(EventBuffer {
                    next_seq: 0,
                    events: VecDeque::new(),
                }),
                max_events,
                max_age,
            }),
        }
    }

    pub fn send(&self, msg: EventMessage) {
        let mut buffer = self.inner.buffer.lock().unwrap();
        buffer.next_seq += 1;
        let event = Arc::new(SequencedEvent {
            seq: buffer.next_seq,
            id: format!("{}-{}", self.inner.boot_id, buffer.next_seq),
            created: Instant::now(),
            message: msg,
        });
        buffer.events.push_back(event.clone());
        while buffer.events.len() > self.inner.max_events
            || buffer
                .events
                .front()
                .is_some_and(|e| e.created.elapsed() > self.inner.max_age)
        {
            buffer.events.pop_front();
        }
        // Broadcasting under the buffer lock lets subscribe_after atomically
        // subscribe + snapshot against it, guaranteeing no duplicated or skipped events
        if let Err(e) = self.inner.sender.send(event) {
            debug!("Nowhere send event: {e}");
        };
    }

    fn parse_event_id(&self, id: &str) -> Option<u64> {
        let (boot, seq) = id.rsplit_once('-')?;
        if boot != self.inner.boot_id {
            return None;
        }
        seq.parse().ok()
    }

    /// Live events, preceded by buffered events with id after `last_event_id` (exclusive).
    /// Unknown or foreign id means no replay, live only.
    pub fn subscribe_after(
        &self,
        last_event_id: Option<&str>,
    ) -> impl Stream<Item = Arc<SequencedEvent>> + Send + 'static {
        let resume = last_event_id.and_then(|id| self.parse_event_id(id));
        let (receiver, replay) = {
            // Same lock send() broadcasts under: every event is atomically either
            // in the snapshot (sent before) or in the receiver queue (sent after),
            // so the chained stream has no duplicates and no gaps
            let buffer = self.inner.buffer.lock().unwrap();
            let receiver = self.inner.sender.subscribe();
            let replay: Vec<_> = match resume {
                Some(after) => buffer
                    .events
                    .iter()
                    .filter(|e| e.seq > after)
                    .cloned()
                    .collect(),
                None => Vec::new(),
            };
            (receiver, replay)
        };
        futures::stream::iter(replay).chain(
            tokio_stream::wrappers::BroadcastStream::new(receiver).filter_map(|r| {
                r.inspect_err(|e| error!("EventHub receiver lags: {e}"))
                    .ok()
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(kind: &str, user: Option<&str>) -> EventMessage {
        EventMessage::message(kind, user.map(str::to_string), json!({"kind": kind}))
    }

    async fn next_event(
        stream: &mut (impl Stream<Item = Arc<SequencedEvent>> + Unpin),
    ) -> Arc<SequencedEvent> {
        tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("timed out waiting for event")
            .expect("stream ended")
    }

    async fn assert_no_event(stream: &mut (impl Stream<Item = Arc<SequencedEvent>> + Unpin)) {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), stream.next())
                .await
                .is_err(),
            "unexpected event"
        );
    }

    #[tokio::test]
    async fn test_events_get_monotonic_boot_prefixed_ids() {
        let hub = EventHub::new();
        let mut stream = Box::pin(hub.subscribe_after(None));
        for _ in 0..3 {
            hub.send(msg("test", Some("alice")));
        }
        let mut previous = 0;
        for _ in 0..3 {
            let event = next_event(&mut stream).await;
            let seq = hub.parse_event_id(&event.id).expect("id parses back");
            assert_eq!(seq, event.seq);
            assert!(seq > previous);
            assert_eq!(event.message.target_user.as_deref(), Some("alice"));
            previous = seq;
        }
    }

    #[tokio::test]
    async fn test_replay_after_id_returns_only_newer_events() {
        let hub = EventHub::new();
        for i in 1..=5 {
            hub.send(msg(&format!("e{i}"), None));
        }
        let resume_id = format!("{}-2", hub.inner.boot_id);
        let mut stream = Box::pin(hub.subscribe_after(Some(&resume_id)));
        for expected in 3..=5 {
            assert_eq!(next_event(&mut stream).await.seq, expected);
        }
        assert_no_event(&mut stream).await;
    }

    #[tokio::test]
    async fn test_replay_chains_into_live_without_gaps_or_duplicates() {
        let hub = EventHub::new();
        for i in 1..=3 {
            hub.send(msg(&format!("e{i}"), None));
        }
        let resume_id = format!("{}-1", hub.inner.boot_id);
        let mut stream = Box::pin(hub.subscribe_after(Some(&resume_id)));
        hub.send(msg("e4", None));
        hub.send(msg("e5", None));
        for expected in 2..=5 {
            assert_eq!(next_event(&mut stream).await.seq, expected);
        }
        assert_no_event(&mut stream).await;
    }

    #[tokio::test]
    async fn test_unknown_or_foreign_boot_id_falls_back_to_live_only() {
        let hub = EventHub::new();
        hub.send(msg("buffered", None));
        let malformed = format!("{}-notanumber", hub.inner.boot_id);
        for bad_id in ["deadbeef-1", "garbage", malformed.as_str()] {
            let mut stream = Box::pin(hub.subscribe_after(Some(bad_id)));
            assert_no_event(&mut stream).await;
            hub.send(msg("live", None));
            assert_eq!(next_event(&mut stream).await.message.kind, "live");
        }
    }

    #[tokio::test]
    async fn test_buffer_prunes_by_count_and_age() {
        let hub = EventHub::with_retention(3, Duration::from_millis(50));
        for i in 1..=5 {
            hub.send(msg(&format!("e{i}"), None));
        }
        let replay_all = format!("{}-0", hub.inner.boot_id);
        let mut stream = Box::pin(hub.subscribe_after(Some(&replay_all)));
        for expected in 3..=5 {
            assert_eq!(next_event(&mut stream).await.seq, expected);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        hub.send(msg("e6", None));
        let mut stream = Box::pin(hub.subscribe_after(Some(&replay_all)));
        assert_eq!(next_event(&mut stream).await.seq, 6);
        assert_no_event(&mut stream).await;
    }
}
