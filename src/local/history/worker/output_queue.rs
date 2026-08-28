use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::TrySendError;

use anyhow::{Result, bail};

use super::{LocalHistoryRuntime, WorkerMessage};
use crate::local::history::store::OutputEvent;

// Raw PTY evidence is useful for audit/debugging, but it is auxiliary. Bound
// each invocation so a pathological command cannot monopolize the history
// writer or grow the audit database without limit. The model-facing session
// output has its own independent delivery path.
const OUTPUT_CAPTURE_MAX_BYTES_PER_INVOCATION: usize = 2 * 1024 * 1024;

// This absorbs only short scheduler/SQLite stalls. If it fills, history capture
// for the affected invocation is explicitly marked incomplete and load-shed;
// model execution and result delivery continue normally.
const OUTPUT_TRANSIENT_OVERFLOW_MAX_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub(super) struct OutputOverflow {
    by_invocation: HashMap<i64, VecDeque<OutputEvent>>,
    bytes: usize,
    captured_bytes: HashMap<i64, usize>,
    incomplete_reasons: HashMap<i64, String>,
}

impl OutputOverflow {
    fn admit_chunk(&mut self, invocation_id: i64, event_bytes: usize) -> bool {
        if self.incomplete_reasons.contains_key(&invocation_id) {
            return false;
        }
        let captured = self.captured_bytes.entry(invocation_id).or_default();
        if captured.saturating_add(event_bytes) > OUTPUT_CAPTURE_MAX_BYTES_PER_INVOCATION {
            self.mark_incomplete(
                invocation_id,
                format!(
                    "Local history output capture exceeded {OUTPUT_CAPTURE_MAX_BYTES_PER_INVOCATION} bytes; remaining output was omitted from history"
                ),
            );
            return false;
        }
        *captured = captured.saturating_add(event_bytes);
        true
    }

    fn mark_incomplete(&mut self, invocation_id: i64, reason: String) {
        self.incomplete_reasons
            .entry(invocation_id)
            .or_insert(reason);
    }

    fn incomplete_reason(&self, invocation_id: i64) -> Option<String> {
        self.incomplete_reasons.get(&invocation_id).cloned()
    }

    fn has_pending(&self, invocation_id: i64) -> bool {
        self.by_invocation
            .get(&invocation_id)
            .is_some_and(|events| !events.is_empty())
    }

    fn push_back(&mut self, invocation_id: i64, event: OutputEvent) -> bool {
        let bytes = output_event_bytes(&event);
        if self.bytes.saturating_add(bytes) > OUTPUT_TRANSIENT_OVERFLOW_MAX_BYTES {
            return false;
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.by_invocation
            .entry(invocation_id)
            .or_default()
            .push_back(event);
        true
    }

    fn push_front_existing(&mut self, invocation_id: i64, event: OutputEvent) {
        self.bytes = self.bytes.saturating_add(output_event_bytes(&event));
        self.by_invocation
            .entry(invocation_id)
            .or_default()
            .push_front(event);
    }

    fn pop_front(&mut self, invocation_id: i64) -> Option<OutputEvent> {
        let event = self
            .by_invocation
            .get_mut(&invocation_id)
            .and_then(VecDeque::pop_front)?;
        self.bytes = self.bytes.saturating_sub(output_event_bytes(&event));
        if self
            .by_invocation
            .get(&invocation_id)
            .is_some_and(VecDeque::is_empty)
        {
            self.by_invocation.remove(&invocation_id);
        }
        Some(event)
    }

    fn discard_pending(&mut self, invocation_id: i64) {
        let events = self
            .by_invocation
            .remove(&invocation_id)
            .unwrap_or_default();
        let bytes = events.iter().map(output_event_bytes).sum::<usize>();
        self.bytes = self.bytes.saturating_sub(bytes);
    }

    fn finish(&mut self, invocation_id: i64) -> Option<String> {
        self.captured_bytes.remove(&invocation_id);
        self.incomplete_reasons.remove(&invocation_id)
    }
}

impl LocalHistoryRuntime {
    pub(super) fn try_enqueue_output_chunk(&self, event: OutputEvent, invocation_id: i64) {
        let event_bytes = output_event_bytes(&event);
        let admitted = self
            .output_overflow
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .admit_chunk(invocation_id, event_bytes);
        if !admitted {
            if let Some(reason) = self
                .output_overflow
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .incomplete_reason(invocation_id)
            {
                self.mark_output_capture_incomplete(invocation_id, reason);
            }
            return;
        }

        if self
            .flush_output_overflow_nonblocking(invocation_id)
            .is_err()
        {
            return;
        }

        let has_pending = self
            .output_overflow
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .has_pending(invocation_id);
        if has_pending {
            self.buffer_output_overflow(event, invocation_id);
            return;
        }

        match self.sender.try_send(WorkerMessage::Output(event)) {
            Ok(()) => {}
            Err(TrySendError::Full(WorkerMessage::Output(event))) => {
                self.buffer_output_overflow(event, invocation_id);
            }
            Err(TrySendError::Full(_)) => unreachable!("output send returned a non-output message"),
            Err(TrySendError::Disconnected(_)) => self.degrade_output_capture(
                invocation_id,
                "Local history output writer is unavailable".to_string(),
            ),
        }
    }

    fn buffer_output_overflow(&self, event: OutputEvent, invocation_id: i64) {
        let mut overflow = self
            .output_overflow
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !overflow.push_back(invocation_id, event) {
            let reason = format!(
                "Local history output queue remained saturated after {OUTPUT_TRANSIENT_OVERFLOW_MAX_BYTES} bytes of transient overflow; remaining output was omitted from history"
            );
            overflow.mark_incomplete(invocation_id, reason.clone());
            drop(overflow);
            self.mark_output_capture_incomplete(invocation_id, reason);
        }
    }

    fn flush_output_overflow_nonblocking(&self, invocation_id: i64) -> Result<()> {
        loop {
            let event = self
                .output_overflow
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front(invocation_id);
            let Some(event) = event else {
                return Ok(());
            };

            match self.sender.try_send(WorkerMessage::Output(event)) {
                Ok(()) => {}
                Err(TrySendError::Full(WorkerMessage::Output(event))) => {
                    self.output_overflow
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push_front_existing(invocation_id, event);
                    return Ok(());
                }
                Err(TrySendError::Full(_)) => {
                    unreachable!("output send returned a non-output message")
                }
                Err(TrySendError::Disconnected(_)) => {
                    let reason = "Local history output writer is unavailable".to_string();
                    self.degrade_output_capture(invocation_id, reason.clone());
                    bail!(reason);
                }
            }
        }
    }

    pub(super) fn enqueue_output_completion(&self, invocation_id: i64, agent_id: Option<String>) {
        // EOF handling is deliberately nonblocking too. If transient backlog
        // still cannot drain, discard only that invocation's history overflow
        // and mark its capture incomplete instead of parking the PTY reader.
        if self
            .flush_output_overflow_nonblocking(invocation_id)
            .is_err()
        {
            return;
        }

        {
            let mut overflow = self
                .output_overflow
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if overflow.has_pending(invocation_id) {
                overflow.discard_pending(invocation_id);
                overflow.mark_incomplete(
                    invocation_id,
                    "Local history output queue was still saturated at command completion; queued tail output was omitted from history".to_string(),
                );
            }
        }

        let reason = self
            .output_overflow
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finish(invocation_id);
        let event = match reason {
            Some(reason) => OutputEvent::Incomplete {
                invocation_id,
                agent_id,
                reason,
            },
            None => OutputEvent::Complete {
                invocation_id,
                agent_id,
            },
        };

        match self.sender.try_send(WorkerMessage::Output(event)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.mark_output_capture_incomplete(
                invocation_id,
                "Local history output completion queue is full".to_string(),
            ),
            Err(TrySendError::Disconnected(_)) => self.degrade_output_capture(
                invocation_id,
                "Local history output writer is unavailable".to_string(),
            ),
        }
    }

    fn degrade_output_capture(&self, invocation_id: i64, reason: String) {
        self.output_overflow
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .mark_incomplete(invocation_id, reason.clone());
        self.health
            .note_capture_incomplete(invocation_id, reason.clone());
        self.health.degrade_nonblocking(reason);
    }

    fn mark_output_capture_incomplete(&self, invocation_id: i64, reason: String) {
        self.health.note_capture_incomplete(invocation_id, reason);
    }
}

fn output_event_bytes(event: &OutputEvent) -> usize {
    match event {
        OutputEvent::Chunk { text, .. } => text.len(),
        OutputEvent::Complete { .. } | OutputEvent::Incomplete { .. } => 0,
    }
}
