use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

pub(crate) const HISTORY_LIVE_EVENT_SCHEMA_VERSION: u32 = 1;
const DEFAULT_EVENT_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct HistoryLiveEvent {
    pub schema_version: u32,
    pub runtime_id: String,
    pub sequence: u64,
    pub emitted_at_ms: i64,
    pub event_type: String,
    pub agent_id: Option<String>,
    pub invocation_id: Option<i64>,
    pub presentation_revision: Option<u32>,
    pub payload: Value,
}

pub(crate) struct HistoryEventHub {
    runtime_id: Arc<str>,
    sequence: AtomicU64,
    sender: broadcast::Sender<HistoryLiveEvent>,
}

impl HistoryEventHub {
    pub(crate) fn new(runtime_id: Arc<str>, capacity: usize) -> Arc<Self> {
        let (sender, _) = broadcast::channel(capacity.max(1));
        Arc::new(Self {
            runtime_id,
            sequence: AtomicU64::new(0),
            sender,
        })
    }

    pub(crate) fn default_capacity() -> usize {
        DEFAULT_EVENT_CAPACITY
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<HistoryLiveEvent> {
        self.sender.subscribe()
    }

    #[cfg(test)]
    pub(crate) fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }

    pub(crate) fn current_sequence(&self) -> u64 {
        self.sequence.load(Ordering::Acquire)
    }

    pub(crate) fn emit_with(
        &self,
        event_type: &str,
        agent_id: Option<&str>,
        invocation_id: Option<i64>,
        presentation_revision: Option<u32>,
        payload: impl FnOnce() -> Value,
    ) {
        if self.sender.receiver_count() == 0 {
            return;
        }
        let sequence = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let event = HistoryLiveEvent {
            schema_version: HISTORY_LIVE_EVENT_SCHEMA_VERSION,
            runtime_id: self.runtime_id.to_string(),
            sequence,
            emitted_at_ms: current_time_ms(),
            event_type: event_type.to_string(),
            agent_id: agent_id.map(str::to_owned),
            invocation_id,
            presentation_revision,
            payload: payload(),
        };
        let _ = self.sender.send(event);
    }
}

fn current_time_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}
