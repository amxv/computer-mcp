use anyhow::{Context, Result, bail};
use rusqlite::params;

use crate::protocol::TerminationReason;
use crate::session::OwnedProcessEnd;

use super::store::{HistoryStore, now_ms};

impl HistoryStore {
    pub(super) fn process_started(&self, invocation_id: i64) -> Result<()> {
        let now = now_ms()?;
        let changed = self
            .lock_connection()
            .execute(
                "UPDATE invocations SET
                    process_state = 'running',
                    process_started_at_ms = COALESCE(process_started_at_ms, ?2),
                    process_ended_at_ms = NULL,
                    process_updated_at_ms = ?2,
                    process_exit_code = NULL,
                    process_termination_reason = NULL,
                    process_incomplete_reason = NULL
                 WHERE id = ?1",
                params![invocation_id, now],
            )
            .with_context(|| {
                format!("failed to persist Local process start for invocation {invocation_id}")
            })?;
        if changed != 1 {
            bail!("missing Local invocation {invocation_id} while persisting process start");
        }
        Ok(())
    }

    pub(super) fn process_ended(&self, invocation_id: i64, end: &OwnedProcessEnd) -> Result<()> {
        let now = now_ms()?;
        let connection = self.lock_connection();
        let changed = if end.is_complete() {
            let termination_reason = end.termination_reason.map(termination_reason_name);
            connection.execute(
                "UPDATE invocations SET
                    process_state = 'exited',
                    process_ended_at_ms = ?2,
                    process_updated_at_ms = ?2,
                    process_exit_code = ?3,
                    process_termination_reason = ?4,
                    process_cwd = COALESCE(?5, process_cwd),
                    process_incomplete_reason = NULL
                 WHERE id = ?1",
                params![
                    invocation_id,
                    now,
                    end.exit_code,
                    termination_reason,
                    end.final_cwd,
                ],
            )
        } else {
            connection.execute(
                "UPDATE invocations SET
                    process_state = 'incomplete',
                    process_ended_at_ms = NULL,
                    process_updated_at_ms = ?2,
                    process_exit_code = NULL,
                    process_termination_reason = NULL,
                    process_cwd = COALESCE(?3, process_cwd),
                    process_incomplete_reason = ?4
                 WHERE id = ?1",
                params![invocation_id, now, end.final_cwd, end.incomplete_reason],
            )
        }
        .with_context(|| {
            format!("failed to persist Local process end for invocation {invocation_id}")
        })?;
        if changed != 1 {
            bail!("missing Local invocation {invocation_id} while persisting process end");
        }
        Ok(())
    }

    pub(super) fn recover_interrupted_process_lifecycle(&self) -> Result<()> {
        let now = now_ms()?;
        self.lock_connection()
            .execute(
                "UPDATE invocations SET
                    process_state = 'incomplete',
                    process_ended_at_ms = NULL,
                    process_updated_at_ms = ?1,
                    process_exit_code = NULL,
                    process_termination_reason = NULL,
                    process_incomplete_reason = COALESCE(
                        process_incomplete_reason,
                        'previous Local runtime ended before final process lifecycle was observed'
                    )
                 WHERE process_state = 'running'",
                [now],
            )
            .context("failed to recover interrupted Local process lifecycle evidence")?;
        Ok(())
    }
}

fn termination_reason_name(reason: TerminationReason) -> &'static str {
    match reason {
        TerminationReason::Exit => "exit",
        TerminationReason::Timeout => "timeout",
        TerminationReason::Killed => "killed",
    }
}
