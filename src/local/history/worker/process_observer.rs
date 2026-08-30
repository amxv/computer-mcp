use std::sync::atomic::Ordering;

use anyhow::{Context, Result};

use crate::session::{OwnedProcess, OwnedProcessEnd, OwnedProcessObserver};

use super::{LocalHistoryRuntime, WorkerMessage};

impl OwnedProcessObserver for LocalHistoryRuntime {
    fn process_started(&self, process: &OwnedProcess) -> Result<()> {
        let invocation_id = process
            .created_by
            .invocation_id
            .context("active Local process is missing durable creator invocation ID")?;

        // Protect the creator immediately in memory. Durable lifecycle writes
        // run on the priority history lane, so process launch never contends
        // with output persistence or retention for a SQLite writer lock.
        let newly_active = self
            .active_process_invocation_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(invocation_id, false)
            .is_none();
        let active_process_count = if newly_active {
            self.active_process_count.fetch_add(1, Ordering::AcqRel) + 1
        } else {
            self.active_process_count.load(Ordering::Acquire)
        };
        let agent_id = process.created_by.agent_id.as_deref().map(str::to_owned);
        let agent_active_process_count = agent_id.as_deref().map(|agent_id| {
            let mut counts = self
                .active_process_counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = counts.entry(agent_id.to_string()).or_default();
            if newly_active {
                *count = count.saturating_add(1);
            }
            *count
        });

        self.enqueue_priority_message(
            WorkerMessage::ProcessStarted {
                invocation_id,
                agent_id,
                active_process_count,
                agent_active_process_count,
            },
            invocation_id,
            "Local process-start history writer is unavailable or saturated",
        );
        Ok(())
    }

    fn process_ended(&self, process: &OwnedProcess, end: &OwnedProcessEnd) -> Result<()> {
        let Some(invocation_id) = process.created_by.invocation_id else {
            return Ok(());
        };

        // Mark the process as ending but keep its creator retention-protected
        // until the priority worker commits the terminal lifecycle row. This
        // closes the race where retention could delete a just-finished command
        // between process observation and durable process-end persistence.
        let was_active = {
            let mut processes = self
                .active_process_invocation_ids
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match processes.get_mut(&invocation_id) {
                Some(ending) if !*ending => {
                    *ending = true;
                    true
                }
                Some(_) => false,
                None => {
                    processes.insert(invocation_id, true);
                    false
                }
            }
        };
        let agent_id = process.created_by.agent_id.as_deref().map(str::to_owned);

        self.enqueue_priority_message(
            WorkerMessage::ProcessEnded {
                invocation_id,
                agent_id,
                end: end.clone(),
                was_active,
            },
            invocation_id,
            "Local process-end history writer is unavailable or saturated",
        );
        Ok(())
    }
}
