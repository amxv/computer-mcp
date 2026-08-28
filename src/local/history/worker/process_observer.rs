use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use serde_json::json;

use crate::local::presentation::PRESENTATION_SCHEMA_VERSION;
use crate::session::{OwnedProcess, OwnedProcessEnd, OwnedProcessObserver};

use super::LocalHistoryRuntime;

impl OwnedProcessObserver for LocalHistoryRuntime {
    fn process_started(&self, process: &OwnedProcess) -> Result<()> {
        let invocation_id = process
            .created_by
            .invocation_id
            .context("active Local process is missing durable creator invocation ID")?;
        if let Err(error) = self.store.process_started(invocation_id) {
            self.health.degrade_nonblocking(format!(
                "process lifecycle start persistence failed: {error}"
            ));
            return Err(error);
        }
        if let Err(error) = self.store.protect_active_process_invocation(invocation_id) {
            self.health.degrade_nonblocking(format!(
                "active process retention protection failed: {error}"
            ));
        }
        self.active_process_invocation_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(invocation_id);
        let active_process_count = self.active_process_count.fetch_add(1, Ordering::AcqRel) + 1;
        let agent_id = process.created_by.agent_id.as_deref();
        let agent_active_process_count = agent_id.map(|agent_id| {
            let mut counts = self
                .active_process_counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = counts.entry(agent_id.to_string()).or_default();
            *count = count.saturating_add(1);
            *count
        });
        let emit_live = self.events.emit_with(
            "process_started",
            agent_id,
            Some(invocation_id),
            Some(invocation_id),
            Some(PRESENTATION_SCHEMA_VERSION),
            || {
                json!({
                    "active_process_count": active_process_count,
                    "agent_active_process_count": agent_active_process_count,
                })
            },
        );
        self.queue_presentation_refresh(
            Some(invocation_id),
            Some(invocation_id),
            "process_started",
            emit_live,
        );
        Ok(())
    }

    fn process_ended(&self, process: &OwnedProcess, end: &OwnedProcessEnd) -> Result<()> {
        let Some(invocation_id) = process.created_by.invocation_id else {
            return Ok(());
        };
        let lifecycle_result = self.store.process_ended(invocation_id, end);
        if let Err(error) = &lifecycle_result {
            self.health
                .degrade_nonblocking(format!("process lifecycle end persistence failed: {error}"));
        }
        let retention_result = self
            .store
            .unprotect_active_process_invocation(invocation_id);
        let was_active = self
            .active_process_invocation_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&invocation_id);
        let active_process_count = if was_active {
            self.active_process_count
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    Some(count.saturating_sub(1))
                })
                .unwrap_or(0)
                .saturating_sub(1)
        } else {
            self.active_process_count.load(Ordering::Acquire)
        };
        let agent_id = process.created_by.agent_id.as_deref();
        let agent_active_process_count = agent_id.map(|agent_id| {
            let mut counts = self
                .active_process_counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = counts.entry(agent_id.to_string()).or_default();
            if was_active {
                *count = count.saturating_sub(1);
            }
            let active = *count;
            if active == 0 {
                counts.remove(agent_id);
            }
            active
        });
        if lifecycle_result.is_ok() {
            self.events.clear_presentation(invocation_id);
            let emit_live = self.events.emit_with(
                "process_ended",
                agent_id,
                Some(invocation_id),
                Some(invocation_id),
                Some(PRESENTATION_SCHEMA_VERSION),
                || {
                    json!({
                        "active_process_count": active_process_count,
                        "agent_active_process_count": agent_active_process_count,
                    })
                },
            );
            // Presentation materialization is observability work. Queue it on
            // the history worker instead of touching SQLite synchronously on
            // the command-result path.
            self.queue_presentation_refresh(
                Some(invocation_id),
                Some(invocation_id),
                "process_ended",
                emit_live,
            );
        }
        if let Err(error) = retention_result {
            self.health
                .degrade_nonblocking(format!("active process retention release failed: {error}"));
        }
        lifecycle_result
    }
}
