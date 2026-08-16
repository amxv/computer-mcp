use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::local::file_evidence::{CompletedFileEvidence, PendingFileEvidence};

use super::store::HistoryStore;

impl HistoryStore {
    pub(super) fn persist_file_evidence_start(
        &self,
        invocation_id: i64,
        evidence: &[PendingFileEvidence],
    ) -> Result<()> {
        if evidence.is_empty() {
            return Ok(());
        }
        let mut connection = self.lock_connection();
        persist_start(&mut connection, invocation_id, evidence)
    }

    pub(super) fn persist_file_evidence_completion(
        &self,
        invocation_id: i64,
        evidence: &[CompletedFileEvidence],
    ) -> Result<()> {
        if evidence.is_empty() {
            return Ok(());
        }
        let mut connection = self.lock_connection();
        let transaction = connection
            .transaction()
            .context("failed to start Local file-evidence completion transaction")?;
        for item in evidence {
            transaction
                .execute(
                    "UPDATE invocation_file_evidence SET
                        after_state = ?3, after_text = ?4, after_reason = ?5,
                        source_after_state = ?6, source_after_text = ?7, source_after_reason = ?8
                     WHERE invocation_id = ?1 AND ordinal = ?2",
                    params![
                        invocation_id,
                        i64::from(item.ordinal),
                        item.after.state(),
                        item.after.text(),
                        item.after.reason(),
                        item.source_after.as_ref().map(|value| value.state()),
                        item.source_after.as_ref().and_then(|value| value.text()),
                        item.source_after.as_ref().and_then(|value| value.reason()),
                    ],
                )
                .with_context(|| {
                    format!(
                        "failed to persist Local file evidence completion for invocation {invocation_id} item {}",
                        item.ordinal
                    )
                })?;
        }
        transaction
            .commit()
            .context("failed to commit Local file-evidence completion transaction")
    }

    pub(super) fn mark_file_evidence_unavailable(
        &self,
        invocation_id: i64,
        reason: &str,
    ) -> Result<()> {
        self.lock_connection()
            .execute(
                "UPDATE invocation_file_evidence SET
                    after_state = CASE WHEN after_state = 'pending' THEN 'unavailable' ELSE after_state END,
                    after_reason = CASE WHEN after_state = 'pending' THEN ?2 ELSE after_reason END
                 WHERE invocation_id = ?1",
                params![invocation_id, reason],
            )
            .with_context(|| {
                format!("failed to mark Local file evidence unavailable for invocation {invocation_id}")
            })?;
        Ok(())
    }

    pub(super) fn recover_interrupted_file_evidence(&self) -> Result<()> {
        self.mark_pending_file_evidence_unavailable(
            "previous Local runtime ended before after-file evidence completed",
        )
    }

    pub(super) fn mark_pending_file_evidence_unavailable(&self, reason: &str) -> Result<()> {
        self.lock_connection()
            .execute(
                "UPDATE invocation_file_evidence SET
                    after_state = 'unavailable',
                    after_reason = COALESCE(after_reason, ?1)
                 WHERE after_state = 'pending'",
                [reason],
            )
            .context("failed to finalize pending Local file evidence")?;
        Ok(())
    }
}

fn persist_start(
    connection: &mut Connection,
    invocation_id: i64,
    evidence: &[PendingFileEvidence],
) -> Result<()> {
    let transaction = connection
        .transaction()
        .context("failed to start Local file-evidence transaction")?;
    for item in evidence {
        transaction
            .execute(
                "INSERT INTO invocation_file_evidence(
                    invocation_id, ordinal, source_kind, operation_hint, path_before, path_after,
                    before_state, before_text, before_reason,
                    destination_before_state, destination_before_text, destination_before_reason,
                    after_state
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'pending')",
                params![
                    invocation_id,
                    i64::from(item.ordinal),
                    item.plan.source.as_str(),
                    item.plan.operation.as_str(),
                    item.plan.path_before.to_string_lossy().as_ref(),
                    item.plan.path_after.to_string_lossy().as_ref(),
                    item.before.state(),
                    item.before.text(),
                    item.before.reason(),
                    item.destination_before.as_ref().map(|value| value.state()),
                    item.destination_before.as_ref().and_then(|value| value.text()),
                    item.destination_before.as_ref().and_then(|value| value.reason()),
                ],
            )
            .with_context(|| {
                format!(
                    "failed to persist Local before-file evidence for invocation {invocation_id} item {}",
                    item.ordinal
                )
            })?;
    }
    transaction
        .commit()
        .context("failed to commit Local before-file evidence")
}
