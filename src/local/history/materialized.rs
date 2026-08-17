use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, params, params_from_iter};
use tracing::warn;

use crate::local::presentation::{
    PRESENTATION_SCHEMA_VERSION, PresentationKind, PresentationRecord, project_diff_lines,
};

use super::events::HistoryEventHub;
use super::store::{HistoryStore, now_ms};
use super::timeline::{ROOTS_CTE, timeline_detail_uncached};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryDiffProjection {
    Summary,
    Full,
}

#[derive(Debug, Clone)]
pub(super) struct MaterializedPresentation {
    pub(super) summary: Arc<PresentationRecord>,
    pub(super) full: Option<Arc<PresentationRecord>>,
    pub(super) serialized_bytes: usize,
}

impl MaterializedPresentation {
    pub(super) fn projected(&self, projection: HistoryDiffProjection) -> Arc<PresentationRecord> {
        match projection {
            HistoryDiffProjection::Summary => self.summary.clone(),
            HistoryDiffProjection::Full => {
                self.full.clone().unwrap_or_else(|| self.summary.clone())
            }
        }
    }
}

impl HistoryStore {
    pub(super) fn refresh_presentation_materialization(
        &self,
        root_invocation_id: i64,
    ) -> Result<Option<MaterializedPresentation>> {
        let connection = self.lock_connection();
        let Some(record) = timeline_detail_uncached(&connection, root_invocation_id)? else {
            return Ok(None);
        };
        let mut materialized = materialize_record(&record);
        materialized.serialized_bytes =
            persist_materialized(&connection, root_invocation_id, &materialized)?;
        Ok(Some(materialized))
    }

    pub(super) fn backfill_presentation_materializations(&self, limit: usize) -> Result<usize> {
        if limit == 0 {
            return Ok(0);
        }
        let connection = self.lock_connection();
        if schema_version(&connection)? < 5 {
            return Ok(0);
        }
        let sql = format!(
            "{ROOTS_CTE}
             SELECT roots.root_id
             FROM roots
             LEFT JOIN presentation_materializations materialized
               ON materialized.root_invocation_id = roots.root_id
              AND materialized.presentation_version = ?1
             WHERE materialized.root_invocation_id IS NULL
             ORDER BY roots.root_started_at_ms DESC, roots.root_id DESC
             LIMIT ?2"
        );
        let mut statement = connection
            .prepare(&sql)
            .context("failed to prepare Local presentation-materialization backfill")?;
        let root_ids = statement
            .query_map(
                params![
                    PRESENTATION_SCHEMA_VERSION,
                    i64::try_from(limit).unwrap_or(i64::MAX),
                ],
                |row| row.get::<_, i64>(0),
            )
            .context("failed to query Local presentation-materialization backfill")?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to decode Local presentation-materialization backfill")?;
        drop(statement);

        let mut materialized_count = 0;
        for root_id in root_ids {
            let Some(record) = timeline_detail_uncached(&connection, root_id)? else {
                continue;
            };
            let materialized = materialize_record(&record);
            let _ = persist_materialized(&connection, root_id, &materialized)?;
            materialized_count += 1;
        }
        Ok(materialized_count)
    }

    pub(super) fn discard_presentation_materializations(&self) -> Result<usize> {
        self.lock_connection()
            .execute("DELETE FROM presentation_materializations", [])
            .context("failed to discard derived Local presentation cache under size pressure")
    }
}

pub(super) fn refresh_live_presentation(
    store: &HistoryStore,
    events: &HistoryEventHub,
    root_invocation_id: i64,
) -> Result<Option<Arc<PresentationRecord>>> {
    let Some(materialized) = store.refresh_presentation_materialization(root_invocation_id)? else {
        return Ok(None);
    };
    let summary = materialized.summary.clone();
    events.set_presentation(root_invocation_id, materialized);
    Ok(Some(summary))
}

pub(super) fn refresh_and_emit_presentation(
    store: &HistoryStore,
    events: &HistoryEventHub,
    root_invocation_id: i64,
    invocation_id: Option<i64>,
    fallback_agent_id: Option<&str>,
    source: &str,
    emit_live: bool,
) {
    match refresh_live_presentation(store, events, root_invocation_id) {
        Ok(Some(record)) if emit_live => events.emit_presentation_updated(
            record.agent_id.as_deref(),
            invocation_id,
            root_invocation_id,
            source,
        ),
        Ok(None) if emit_live => events.emit_presentation_updated(
            fallback_agent_id,
            invocation_id,
            root_invocation_id,
            source,
        ),
        Ok(_) => {}
        Err(error) => {
            warn!(
                event = "local_presentation_materialization_failed",
                root_invocation_id,
                error = %error,
            );
            if emit_live {
                events.emit_presentation_updated(
                    fallback_agent_id,
                    invocation_id,
                    root_invocation_id,
                    source,
                );
            }
        }
    }
}

pub(super) fn load_materialized(
    connection: &Connection,
    root_ids: &[i64],
    projection: HistoryDiffProjection,
) -> Result<HashMap<i64, PresentationRecord>> {
    if root_ids.is_empty() || schema_version(connection)? < 5 {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat_n("?", root_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let payload_expression = match projection {
        HistoryDiffProjection::Summary => "summary_json",
        HistoryDiffProjection::Full => "COALESCE(full_json, summary_json)",
    };
    let sql = format!(
        "SELECT root_invocation_id, presentation_version, {payload_expression}
         FROM presentation_materializations
         WHERE root_invocation_id IN ({placeholders})"
    );
    let parameters = root_ids
        .iter()
        .copied()
        .map(SqlValue::Integer)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare Local presentation-materialization query")?;
    let mut rows = statement
        .query(params_from_iter(parameters))
        .context("failed to query Local presentation materializations")?;
    let mut result = HashMap::with_capacity(root_ids.len());
    while let Some(row) = rows
        .next()
        .context("failed to iterate Local presentation materializations")?
    {
        let root_id: i64 = row.get(0)?;
        let version: u32 = row.get(1)?;
        if version != PRESENTATION_SCHEMA_VERSION {
            continue;
        }
        let json: String = row.get(2)?;
        match serde_json::from_str::<PresentationRecord>(&json) {
            Ok(record) => {
                result.insert(root_id, record);
            }
            Err(error) => warn!(
                event = "local_presentation_materialization_decode_failed",
                root_invocation_id = root_id,
                error = %error,
            ),
        }
    }
    Ok(result)
}

fn materialize_record(record: &PresentationRecord) -> MaterializedPresentation {
    let summary = Arc::new(project_diff_lines(record, false));
    let full = matches!(record.kind, PresentationKind::FileChanges { .. })
        .then(|| Arc::new(record.clone()));
    MaterializedPresentation {
        summary,
        full,
        serialized_bytes: 0,
    }
}

fn persist_materialized(
    connection: &Connection,
    root_invocation_id: i64,
    materialized: &MaterializedPresentation,
) -> Result<usize> {
    let summary_json = serde_json::to_string(materialized.summary.as_ref())
        .context("failed to serialize Local presentation summary")?;
    let full_json = materialized
        .full
        .as_ref()
        .map(|record| serde_json::to_string(record.as_ref()))
        .transpose()
        .context("failed to serialize Local presentation diff body")?;
    connection
        .execute(
            "INSERT INTO presentation_materializations(
                root_invocation_id, presentation_version, materialized_at_ms, summary_json, full_json
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(root_invocation_id) DO UPDATE SET
                presentation_version = excluded.presentation_version,
                materialized_at_ms = excluded.materialized_at_ms,
                summary_json = excluded.summary_json,
                full_json = excluded.full_json",
            params![
                root_invocation_id,
                PRESENTATION_SCHEMA_VERSION,
                now_ms()?,
                summary_json,
                full_json,
            ],
        )
        .with_context(|| {
            format!(
                "failed to persist Local presentation materialization for root {root_invocation_id}"
            )
        })?;
    Ok(summary_json.len() + full_json.as_ref().map_or(0, String::len))
}

fn schema_version(connection: &Connection) -> Result<u32> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .context("failed to inspect Local history schema for presentation materializations")
}
