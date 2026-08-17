use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::invocation::{InvocationContext, InvocationOutcome};
use crate::local::presentation::{PRESENTATION_SCHEMA_VERSION, presentation_id_for_root};

use super::materialized::{HistoryDiffProjection, MaterializedPresentation};

pub(crate) const HISTORY_LIVE_EVENT_SCHEMA_VERSION: u32 = 2;
const DEFAULT_EVENT_CAPACITY: usize = 256;
const LIVE_PRESENTATION_CACHE_MAX_ENTRIES: usize = 128;
const LIVE_PRESENTATION_CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct LivePresentationCache {
    entries: HashMap<i64, MaterializedPresentation>,
    order: VecDeque<i64>,
    bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct HistoryLiveEvent {
    pub schema_version: u32,
    pub runtime_id: String,
    pub sequence: u64,
    pub emitted_at_ms: i64,
    pub event_type: String,
    pub agent_id: Option<String>,
    pub invocation_id: Option<i64>,
    pub presentation_id: Option<String>,
    pub presentation_revision: Option<u32>,
    pub payload: Value,
}

pub(crate) struct HistoryEventHub {
    runtime_id: Arc<str>,
    sequence: AtomicU64,
    sender: broadcast::Sender<HistoryLiveEvent>,
    presentations: RwLock<LivePresentationCache>,
}

impl HistoryEventHub {
    pub(crate) fn new(runtime_id: Arc<str>, capacity: usize) -> Arc<Self> {
        let (sender, _) = broadcast::channel(capacity.max(1));
        Arc::new(Self {
            runtime_id,
            sequence: AtomicU64::new(0),
            sender,
            presentations: RwLock::new(LivePresentationCache::default()),
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

    pub(crate) fn has_subscribers(&self) -> bool {
        self.sender.receiver_count() > 0
    }

    pub(crate) fn set_presentation(
        &self,
        root_invocation_id: i64,
        presentation: MaterializedPresentation,
    ) {
        let mut presentations = self
            .presentations
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous) = presentations.entries.remove(&root_invocation_id) {
            presentations.bytes = presentations
                .bytes
                .saturating_sub(previous.serialized_bytes);
            presentations.order.retain(|id| *id != root_invocation_id);
        }
        presentations.bytes = presentations
            .bytes
            .saturating_add(presentation.serialized_bytes);
        presentations
            .entries
            .insert(root_invocation_id, presentation);
        presentations.order.push_back(root_invocation_id);
        while presentations.entries.len() > 1
            && (presentations.entries.len() > LIVE_PRESENTATION_CACHE_MAX_ENTRIES
                || presentations.bytes > LIVE_PRESENTATION_CACHE_MAX_BYTES)
        {
            let Some(evicted) = presentations.order.pop_front() else {
                break;
            };
            if let Some(previous) = presentations.entries.remove(&evicted) {
                presentations.bytes = presentations
                    .bytes
                    .saturating_sub(previous.serialized_bytes);
            }
        }
    }

    pub(crate) fn clear_presentation(&self, root_invocation_id: i64) {
        let mut presentations = self
            .presentations
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous) = presentations.entries.remove(&root_invocation_id) {
            presentations.bytes = presentations
                .bytes
                .saturating_sub(previous.serialized_bytes);
            presentations.order.retain(|id| *id != root_invocation_id);
        }
    }

    pub(crate) fn presentation(
        &self,
        root_invocation_id: i64,
        projection: HistoryDiffProjection,
    ) -> Option<Arc<crate::local::presentation::PresentationRecord>> {
        self.presentations
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .get(&root_invocation_id)
            .map(|presentation| presentation.projected(projection))
    }

    pub(crate) fn emit_with(
        &self,
        event_type: &str,
        agent_id: Option<&str>,
        invocation_id: Option<i64>,
        presentation_root_invocation_id: Option<i64>,
        presentation_revision: Option<u32>,
        payload: impl FnOnce() -> Value,
    ) -> bool {
        if self.sender.receiver_count() == 0 {
            return false;
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
            presentation_id: presentation_root_invocation_id.map(presentation_id_for_root),
            presentation_revision,
            payload: payload(),
        };
        self.sender.send(event).is_ok()
    }

    pub(crate) fn emit_invocation_completion(
        &self,
        context: &InvocationContext,
        outcome: &InvocationOutcome,
        presentation_root_invocation_id: Option<i64>,
    ) -> bool {
        let invocation_id = context.invocation_id;
        let agent_id = context.agent_id.as_deref();
        let outcome_kind = match outcome {
            InvocationOutcome::Success(_) => "success",
            InvocationOutcome::Error(_) => "error",
        };
        self.emit_with(
            "invocation_completed",
            agent_id,
            invocation_id,
            presentation_root_invocation_id,
            Some(PRESENTATION_SCHEMA_VERSION),
            || json!({"outcome": outcome_kind}),
        )
    }

    pub(crate) fn emit_presentation_updated(
        &self,
        agent_id: Option<&str>,
        invocation_id: Option<i64>,
        presentation_root_invocation_id: i64,
        source: &str,
    ) {
        self.emit_with(
            "presentation_updated",
            agent_id,
            invocation_id,
            Some(presentation_root_invocation_id),
            Some(PRESENTATION_SCHEMA_VERSION),
            || json!({"source": source}),
        );
    }
}

fn current_time_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}
