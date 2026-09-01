use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::OptionalExtension;

use crate::local::presentation::StreamingDisplaySanitizer;

use super::LocalHistoryReader;
use super::query::{HistoryOutputChunk, open_read_only, result_output_for_invocation};
use super::schema::verify_readable_schema;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryDisplayOutputPage {
    pub chunks: Vec<HistoryOutputChunk>,
    pub next_cursor: Option<u64>,
    pub display_state: String,
    pub display_reason: Option<String>,
}

impl LocalHistoryReader {
    /// Return display-safe PTY chunks while retaining ANSI/OSC parser state
    /// across the exact durable chunk boundaries.
    ///
    /// A later cursor still replays earlier raw chunks internally so parser
    /// state is correct, but only the requested bounded page is returned.
    pub(crate) fn display_output_page(
        path: &Path,
        invocation_id: i64,
        cursor: u64,
        limit: usize,
    ) -> Result<Option<HistoryDisplayOutputPage>> {
        if !path.exists() {
            return Ok(None);
        }
        let connection = open_read_only(path)?;
        verify_readable_schema(&connection)?;
        let invocation = connection
            .query_row(
                "SELECT COALESCE(completed_at_ms, started_at_ms)
                 FROM invocations WHERE id = ?1",
                [invocation_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .context("failed to check Local invocation for display output page")?;
        let Some(result_observed_at_ms) = invocation else {
            return Ok(None);
        };

        let limit = limit.max(1);
        let mut statement = connection
            .prepare(
                "SELECT sequence, observed_at_ms, text
                 FROM invocation_output_chunks
                 WHERE invocation_id = ?1
                 ORDER BY sequence ASC",
            )
            .context("failed to prepare Local display output replay")?;
        let mut rows = statement
            .query([invocation_id])
            .context("failed to query Local display output replay")?;

        let mut sanitizer = StreamingDisplaySanitizer::new();
        let mut expected_sequence = 0_u64;
        let mut chunks = Vec::with_capacity(limit);
        let mut next_cursor = None;

        while let Some(row) = rows
            .next()
            .context("failed to iterate Local display output replay")?
        {
            let stored_sequence: i64 = row.get(0).context("failed to decode output sequence")?;
            let Ok(sequence) = u64::try_from(stored_sequence) else {
                return Ok(Some(display_unavailable(
                    "stored PTY output has an invalid negative sequence",
                )));
            };
            if sequence != expected_sequence {
                return Ok(Some(display_unavailable(format!(
                    "stored PTY output sequence is incomplete at {expected_sequence} (next retained sequence is {sequence})"
                ))));
            }
            expected_sequence = expected_sequence.saturating_add(1);

            let observed_at_ms = row
                .get(1)
                .context("failed to decode output observation timestamp")?;
            let raw: String = row.get(2).context("failed to decode output text")?;

            if sequence < cursor {
                let _ = sanitizer.push(&raw);
                continue;
            }
            if chunks.len() >= limit {
                next_cursor = Some(sequence);
                break;
            }

            chunks.push(HistoryOutputChunk {
                sequence,
                observed_at_ms,
                text: sanitizer.push(&raw),
            });
        }

        if expected_sequence == 0
            && cursor == 0
            && let Some(output) = result_output_for_invocation(&connection, invocation_id)?
            && !output.is_empty()
        {
            chunks.push(HistoryOutputChunk {
                sequence: 0,
                observed_at_ms: result_observed_at_ms,
                text: sanitizer.push(&output),
            });
        }

        Ok(Some(HistoryDisplayOutputPage {
            chunks,
            next_cursor,
            display_state: "available".to_string(),
            display_reason: None,
        }))
    }
}

fn display_unavailable(reason: impl Into<String>) -> HistoryDisplayOutputPage {
    HistoryDisplayOutputPage {
        chunks: Vec::new(),
        next_cursor: None,
        display_state: "unavailable".to_string(),
        display_reason: Some(reason.into()),
    }
}
