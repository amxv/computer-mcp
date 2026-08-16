use std::collections::{HashMap, VecDeque};

use anyhow::{Result, bail};
use std::sync::mpsc::TrySendError;

use super::{LocalHistoryRuntime, WorkerMessage};
use crate::local::history::store::OutputEvent;

// This is intentionally much smaller than the main bounded queue's worst-case
// byte footprint. It absorbs only short scheduler/SQLite stalls without making
// sustained evidence backlogs unbounded or silently lossy.
const OUTPUT_TRANSIENT_OVERFLOW_MAX_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub(super) struct OutputOverflow {
    by_invocation: HashMap<i64, VecDeque<OutputEvent>>,
    bytes: usize,
}

impl OutputOverflow {
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

    fn take(&mut self, invocation_id: i64) -> VecDeque<OutputEvent> {
        let events = self
            .by_invocation
            .remove(&invocation_id)
            .unwrap_or_default();
        let bytes = events.iter().map(output_event_bytes).sum::<usize>();
        self.bytes = self.bytes.saturating_sub(bytes);
        events
    }
}

impl LocalHistoryRuntime {
    pub(super) fn try_enqueue_output_chunk(&self, event: OutputEvent, invocation_id: i64) {
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
        let buffered = self
            .output_overflow
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(invocation_id, event);
        if !buffered {
            self.degrade_output_capture(
                invocation_id,
                format!(
                    "Local history output queue is full; transient overflow exceeded {OUTPUT_TRANSIENT_OVERFLOW_MAX_BYTES} bytes"
                ),
            );
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

    fn flush_output_overflow_blocking(&self, invocation_id: i64) -> Result<()> {
        let pending = self
            .output_overflow
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take(invocation_id);
        for event in pending {
            self.sender
                .send(WorkerMessage::Output(event))
                .map_err(|_| anyhow::anyhow!("Local history output writer is unavailable"))?;
        }
        Ok(())
    }

    pub(super) fn enqueue_output_completion(&self, message: WorkerMessage, invocation_id: i64) {
        // Output chunks stay nonblocking while the command runs. At EOF the
        // detached reader may wait for bounded backlog to drain, ensuring all
        // accepted chunks are queued before the terminal completion marker.
        if self.flush_output_overflow_blocking(invocation_id).is_err()
            || self.sender.send(message).is_err()
        {
            self.degrade_output_capture(
                invocation_id,
                "Local history output writer is unavailable".to_string(),
            );
        }
    }

    fn degrade_output_capture(&self, invocation_id: i64, reason: String) {
        self.health.note_capture_incomplete(invocation_id);
        self.health.degrade_nonblocking(reason);
    }
}

fn output_event_bytes(event: &OutputEvent) -> usize {
    match event {
        OutputEvent::Chunk { text, .. } => text.len(),
        OutputEvent::Complete { .. } => 0,
    }
}
