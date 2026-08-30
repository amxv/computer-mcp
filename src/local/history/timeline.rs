use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::local::presentation::{
    PresentationEvidence, PresentationInput, PresentationOrphanPollInput,
    PresentationPollAggregateInput, PresentationRecord, build_presentation_inputs,
    sanitize_display_text,
};

use super::materialized::load_materialized;
use super::query::{map_file_evidence_at, open_read_only};
use super::schema::readable_schema_version;
use super::timeline_polls::{load_handle_poll_aggregates, load_parent_poll_aggregates};
use super::{HistoryDiffProjection, HistoryTimelineCursor, LocalHistoryReader};

pub(super) const ROOTS_CTE: &str = r#"
WITH roots(root_id, representative_id, root_started_at_ms, orphan_kind, orphan_handle) AS (
    SELECT i.id, i.id, i.started_at_ms, 0, NULL
    FROM invocations i
    WHERE NOT (
        i.tool_name = 'write_stdin'
        AND i.continuation_kind = 'poll'
    )

    UNION ALL

    SELECT
        MIN(p.id),
        (
            SELECT p2.id
            FROM invocations p2
            WHERE p2.tool_name = 'write_stdin'
              AND p2.continuation_kind = 'poll'
              AND p2.target_created_by_invocation_id IS NULL
              AND p2.target_session_handle = p.target_session_handle
            ORDER BY p2.started_at_ms DESC, p2.id DESC
            LIMIT 1
        ),
        MIN(p.started_at_ms),
        1,
        p.target_session_handle
    FROM invocations p
    WHERE p.tool_name = 'write_stdin'
      AND p.continuation_kind = 'poll'
      AND p.target_created_by_invocation_id IS NULL
      AND p.target_session_handle IS NOT NULL
    GROUP BY p.target_session_handle

    UNION ALL

    SELECT p.id, p.id, p.started_at_ms, 2, NULL
    FROM invocations p
    WHERE p.tool_name = 'write_stdin'
      AND p.continuation_kind = 'poll'
      AND p.target_created_by_invocation_id IS NULL
      AND p.target_session_handle IS NULL
)
"#;

/// Deliberately omits `result_json` and all PTY output tables. The canonical
/// timeline hot path consumes denormalized result/lifecycle columns only.
pub(super) const LEAN_TIMELINE_INVOCATION_SELECT: &str = r#"
SELECT id, agent_id, tool_name, args_json,
       declared_workdir_exact, declared_workdir_normalized, is_new_workdir,
       started_at_ms, completed_at_ms, duration_ms, outcome_kind, error_text,
       evidence_state, evidence_reason, capture_state, capture_reason,
       target_session_handle, target_created_by_agent_id,
       target_created_by_invocation_id, continuation_kind, cross_agent,
       result_status, result_cwd, result_session_handle, result_exit_code,
       result_termination_reason, process_state, process_started_at_ms,
       process_ended_at_ms, process_exit_code, process_termination_reason,
       process_cwd, process_incomplete_reason
FROM invocations
"#;

#[derive(Debug, Clone)]
pub(crate) enum HistoryTimelineMode {
    History {
        before_ms: Option<i64>,
    },
    Recovery {
        since_ms: i64,
        active_process_invocation_ids: Vec<i64>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct HistoryTimelineQuery {
    pub limit: usize,
    pub cursor: Option<HistoryTimelineCursor>,
    pub agent_id: Option<String>,
    pub normalized_workdir: Option<String>,
    pub diff_projection: HistoryDiffProjection,
    pub mode: HistoryTimelineMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct HistoryTimelinePage {
    pub records: Vec<PresentationRecord>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct HistoryTimelineCheckpoint {
    pub invocation_id: i64,
    pub checkpoint_kind: String,
    pub agent_id: Option<String>,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub status: String,
    pub cross_agent: Option<bool>,
    pub evidence_state: String,
    pub capture_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct HistoryTimelineCheckpointPage {
    pub checkpoints: Vec<HistoryTimelineCheckpoint>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
struct TimelineRoot {
    root_id: i64,
    representative_id: i64,
    started_at_ms: i64,
    orphan_kind: i64,
    orphan_handle: Option<String>,
    changed_at_ms: Option<i64>,
}

impl LocalHistoryReader {
    pub(crate) fn timeline(
        path: &Path,
        query: &HistoryTimelineQuery,
    ) -> Result<HistoryTimelinePage> {
        if !path.exists() {
            return Ok(HistoryTimelinePage {
                records: Vec::new(),
                has_more: false,
                next_cursor: None,
            });
        }
        let connection = open_read_only(path)?;
        require_timeline_schema(&connection)?;
        let requested = query.limit.max(1);
        let mut roots = match &query.mode {
            HistoryTimelineMode::History { before_ms } => {
                query_history_roots(&connection, query, *before_ms, requested.saturating_add(1))?
            }
            HistoryTimelineMode::Recovery {
                since_ms,
                active_process_invocation_ids,
            } => query_recovery_roots(
                &connection,
                query,
                *since_ms,
                active_process_invocation_ids,
                requested.saturating_add(1),
            )?,
        };
        let has_more = roots.len() > requested;
        if has_more {
            roots.pop();
        }
        let next_cursor = has_more
            .then(|| root_cursor(query, roots.last().expect("non-empty paginated root page")))
            .map(|cursor| cursor.encode());

        if matches!(query.mode, HistoryTimelineMode::History { .. }) {
            roots.reverse();
        }
        let records = hydrate_roots(&connection, &roots, query.diff_projection)?;
        Ok(HistoryTimelinePage {
            records,
            has_more,
            next_cursor,
        })
    }

    pub(crate) fn timeline_detail(
        path: &Path,
        presentation_root_id: i64,
    ) -> Result<Option<PresentationRecord>> {
        if !path.exists() {
            return Ok(None);
        }
        let connection = open_read_only(path)?;
        require_timeline_schema(&connection)?;
        let sql = format!(
            "{ROOTS_CTE}
             SELECT root_id, representative_id, root_started_at_ms, orphan_kind, orphan_handle
             FROM roots WHERE root_id = ?1 LIMIT 1"
        );
        let root = connection
            .query_row(&sql, [presentation_root_id], map_root_without_change)
            .optional()
            .context("failed to query canonical Local timeline detail root")?;
        let Some(root) = root else {
            return Ok(None);
        };
        let mut materialized =
            load_materialized(&connection, &[root.root_id], HistoryDiffProjection::Full)?;
        if let Some(record) = materialized.remove(&root.root_id) {
            return Ok(Some(record));
        }
        Ok(timeline_detail_uncached_from_root(&connection, root)?.into())
    }

    pub(crate) fn timeline_details(
        path: &Path,
        presentation_root_ids: &[i64],
    ) -> Result<Vec<PresentationRecord>> {
        if !path.exists() || presentation_root_ids.is_empty() {
            return Ok(Vec::new());
        }
        let connection = open_read_only(path)?;
        require_timeline_schema(&connection)?;
        let placeholders = std::iter::repeat_n("?", presentation_root_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "{ROOTS_CTE}
             SELECT root_id, representative_id, root_started_at_ms, orphan_kind, orphan_handle
             FROM roots WHERE root_id IN ({placeholders})"
        );
        let parameters = presentation_root_ids
            .iter()
            .copied()
            .map(SqlValue::Integer)
            .collect::<Vec<_>>();
        let mut statement = connection
            .prepare(&sql)
            .context("failed to prepare canonical Local timeline batch-detail roots")?;
        let roots = statement
            .query_map(params_from_iter(parameters), map_root_without_change)
            .context("failed to query canonical Local timeline batch-detail roots")?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to decode canonical Local timeline batch-detail roots")?;
        let mut by_id = roots
            .into_iter()
            .map(|root| (root.root_id, root))
            .collect::<HashMap<_, _>>();
        let ordered = presentation_root_ids
            .iter()
            .filter_map(|root_id| by_id.remove(root_id))
            .collect::<Vec<_>>();
        hydrate_roots(&connection, &ordered, HistoryDiffProjection::Full)
    }

    pub(crate) fn timeline_checkpoints(
        path: &Path,
        presentation_root_id: i64,
        limit: usize,
        cursor: Option<&HistoryTimelineCursor>,
    ) -> Result<Option<HistoryTimelineCheckpointPage>> {
        if !path.exists() {
            return Ok(None);
        }
        if let Some(cursor) = cursor
            && !cursor.matches_checkpoints(presentation_root_id)
        {
            bail!("timeline cursor does not belong to this checkpoint query");
        }
        let connection = open_read_only(path)?;
        require_timeline_schema(&connection)?;
        let root_sql = format!(
            "{ROOTS_CTE}
             SELECT root_id, representative_id, root_started_at_ms, orphan_kind, orphan_handle
             FROM roots WHERE root_id = ?1 LIMIT 1"
        );
        let root = connection
            .query_row(&root_sql, [presentation_root_id], map_root_without_change)
            .optional()
            .context("failed to query canonical Local checkpoint root")?;
        let Some(root) = root else {
            return Ok(None);
        };

        let mut parameters = Vec::<SqlValue>::new();
        let predicate = match root.orphan_kind {
            0 => {
                parameters.push(SqlValue::Integer(root.root_id));
                parameters.push(SqlValue::Integer(root.root_id));
                "(i.id = ? OR (i.target_created_by_invocation_id = ? AND i.continuation_kind = 'poll'))"
                    .to_string()
            }
            1 => {
                parameters.push(SqlValue::Text(
                    root.orphan_handle
                        .clone()
                        .context("orphan checkpoint root is missing its session handle")?,
                ));
                "(i.target_created_by_invocation_id IS NULL AND i.continuation_kind = 'poll' AND i.target_session_handle = ?)"
                    .to_string()
            }
            2 => {
                parameters.push(SqlValue::Integer(root.root_id));
                "i.id = ?".to_string()
            }
            other => bail!("unsupported canonical timeline orphan kind {other}"),
        };
        let mut sql = format!(
            "SELECT i.id, i.tool_name, i.continuation_kind, i.agent_id,
                    i.started_at_ms, i.completed_at_ms, i.result_status,
                    i.outcome_kind, i.cross_agent, i.evidence_state, i.capture_state
             FROM invocations i WHERE {predicate}"
        );
        if let Some(HistoryTimelineCursor::Checkpoints {
            started_at_ms,
            invocation_id,
            ..
        }) = cursor
        {
            sql.push_str(" AND (i.started_at_ms > ? OR (i.started_at_ms = ? AND i.id > ?))");
            parameters.push(SqlValue::Integer(*started_at_ms));
            parameters.push(SqlValue::Integer(*started_at_ms));
            parameters.push(SqlValue::Integer(*invocation_id));
        }
        let requested = limit.max(1);
        sql.push_str(" ORDER BY i.started_at_ms ASC, i.id ASC LIMIT ?");
        parameters.push(SqlValue::Integer(
            i64::try_from(requested.saturating_add(1)).unwrap_or(i64::MAX),
        ));
        let mut statement = connection
            .prepare(&sql)
            .context("failed to prepare canonical Local checkpoint query")?;
        let mut checkpoints = statement
            .query_map(params_from_iter(parameters), |row| {
                let invocation_id: i64 = row.get(0)?;
                let tool_name: String = row.get(1)?;
                let continuation_kind: Option<String> = row.get(2)?;
                let completed_at_ms: Option<i64> = row.get(5)?;
                let result_status: Option<String> = row.get(6)?;
                let outcome_kind: Option<String> = row.get(7)?;
                Ok(HistoryTimelineCheckpoint {
                    invocation_id,
                    checkpoint_kind: if tool_name == "write_stdin"
                        && continuation_kind.as_deref() == Some("poll")
                    {
                        "poll".to_string()
                    } else {
                        "initial".to_string()
                    },
                    agent_id: row.get(3)?,
                    started_at_ms: row.get(4)?,
                    completed_at_ms,
                    status: result_status.unwrap_or_else(|| {
                        if outcome_kind.as_deref() == Some("error") {
                            "failed".to_string()
                        } else if completed_at_ms.is_none() {
                            "in_progress".to_string()
                        } else {
                            "completed".to_string()
                        }
                    }),
                    cross_agent: row.get::<_, Option<i64>>(8)?.map(|value| value != 0),
                    evidence_state: row.get(9)?,
                    capture_state: row.get(10)?,
                })
            })
            .context("failed to query canonical Local checkpoints")?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to decode canonical Local checkpoints")?;
        let has_more = checkpoints.len() > requested;
        if has_more {
            checkpoints.pop();
        }
        let next_cursor = has_more.then(|| {
            let last = checkpoints
                .last()
                .expect("non-empty paginated checkpoint page");
            HistoryTimelineCursor::Checkpoints {
                started_at_ms: last.started_at_ms,
                invocation_id: last.invocation_id,
                presentation_root_id,
            }
            .encode()
        });
        Ok(Some(HistoryTimelineCheckpointPage {
            checkpoints,
            has_more,
            next_cursor,
        }))
    }
}

fn require_timeline_schema(connection: &Connection) -> Result<()> {
    let version = readable_schema_version(connection)?;
    if version < 4 {
        bail!(
            "canonical Local timeline requires history schema v4; running Local migrates older stores before serving observation"
        );
    }
    Ok(())
}

fn query_history_roots(
    connection: &Connection,
    query: &HistoryTimelineQuery,
    before_ms: Option<i64>,
    sql_limit: usize,
) -> Result<Vec<TimelineRoot>> {
    if let Some(cursor) = query.cursor.as_ref()
        && !cursor.matches_history(before_ms)
    {
        bail!("timeline cursor does not belong to this history query");
    }
    let (roots_cte, mut parameters) = roots_cte_for_query(query);
    let mut sql = format!(
        "{roots_cte}
         SELECT roots.root_id, roots.representative_id, roots.root_started_at_ms,
                roots.orphan_kind, roots.orphan_handle
         FROM roots
         WHERE 1 = 1"
    );
    if let Some(before_ms) = before_ms {
        sql.push_str(" AND roots.root_started_at_ms < ?");
        parameters.push(SqlValue::Integer(before_ms));
    }
    if let Some(HistoryTimelineCursor::History {
        started_at_ms,
        root_id,
        ..
    }) = query.cursor.as_ref()
    {
        sql.push_str(
            " AND (roots.root_started_at_ms < ? OR (roots.root_started_at_ms = ? AND roots.root_id < ?))",
        );
        parameters.push(SqlValue::Integer(*started_at_ms));
        parameters.push(SqlValue::Integer(*started_at_ms));
        parameters.push(SqlValue::Integer(*root_id));
    }
    sql.push_str(" ORDER BY roots.root_started_at_ms DESC, roots.root_id DESC LIMIT ?");
    parameters.push(SqlValue::Integer(
        i64::try_from(sql_limit).unwrap_or(i64::MAX),
    ));
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare canonical Local history timeline root query")?;
    statement
        .query_map(params_from_iter(parameters), map_root_without_change)
        .context("failed to query canonical Local history timeline roots")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode canonical Local history timeline roots")
}

fn query_recovery_roots(
    connection: &Connection,
    query: &HistoryTimelineQuery,
    since_ms: i64,
    active_ids: &[i64],
    sql_limit: usize,
) -> Result<Vec<TimelineRoot>> {
    if let Some(cursor) = query.cursor.as_ref()
        && !cursor.matches_recovery(since_ms)
    {
        bail!("timeline cursor does not belong to this recovery query");
    }
    let active_clause = if active_ids.is_empty() {
        "0".to_string()
    } else {
        format!(
            "root_changes.root_id IN ({})",
            active_ids
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let (roots_cte, mut parameters) = roots_cte_for_query(query);
    let sql = format!(
        "{roots_cte},
         root_changes AS (
            SELECT roots.*,
                   representative.completed_at_ms AS representative_completed_at_ms,
                   CASE roots.orphan_kind
                     WHEN 0 THEN max(
                         representative.started_at_ms,
                         coalesce(representative.completed_at_ms, representative.started_at_ms),
                         coalesce(representative.process_updated_at_ms, representative.started_at_ms),
                         coalesce((
                             SELECT max(max(
                                 p.started_at_ms,
                                 coalesce(p.completed_at_ms, p.started_at_ms),
                                 coalesce(p.process_updated_at_ms, p.started_at_ms)
                             ))
                             FROM invocations p
                             WHERE p.target_created_by_invocation_id = roots.root_id
                               AND p.continuation_kind = 'poll'
                         ), representative.started_at_ms)
                     )
                     WHEN 1 THEN coalesce((
                         SELECT max(max(
                             p.started_at_ms,
                             coalesce(p.completed_at_ms, p.started_at_ms),
                             coalesce(p.process_updated_at_ms, p.started_at_ms)
                         ))
                         FROM invocations p
                         WHERE p.target_created_by_invocation_id IS NULL
                           AND p.continuation_kind = 'poll'
                           AND p.target_session_handle = roots.orphan_handle
                     ), roots.root_started_at_ms)
                     ELSE max(
                         representative.started_at_ms,
                         coalesce(representative.completed_at_ms, representative.started_at_ms),
                         coalesce(representative.process_updated_at_ms, representative.started_at_ms)
                     )
                   END AS changed_at_ms
            FROM roots
            JOIN invocations representative ON representative.id = roots.representative_id
         ),
         recoverable AS (
            SELECT root_changes.*,
                   CASE
                     WHEN representative_completed_at_ms IS NULL OR {active_clause}
                       THEN max(changed_at_ms, ?)
                     ELSE changed_at_ms
                   END AS effective_changed_at_ms
            FROM root_changes
         )
         SELECT recoverable.root_id, recoverable.representative_id,
                recoverable.root_started_at_ms, recoverable.orphan_kind,
                recoverable.orphan_handle, recoverable.effective_changed_at_ms
         FROM recoverable
         WHERE recoverable.effective_changed_at_ms >= ?"
    );
    let mut sql = sql;
    parameters.push(SqlValue::Integer(since_ms));
    parameters.push(SqlValue::Integer(since_ms));
    if let Some(HistoryTimelineCursor::Recovery {
        changed_at_ms,
        root_id,
        ..
    }) = query.cursor.as_ref()
    {
        sql.push_str(
            " AND (recoverable.effective_changed_at_ms > ? OR (recoverable.effective_changed_at_ms = ? AND recoverable.root_id > ?))",
        );
        parameters.push(SqlValue::Integer(*changed_at_ms));
        parameters.push(SqlValue::Integer(*changed_at_ms));
        parameters.push(SqlValue::Integer(*root_id));
    }
    sql.push_str(
        " ORDER BY recoverable.effective_changed_at_ms ASC, recoverable.root_id ASC LIMIT ?",
    );
    parameters.push(SqlValue::Integer(
        i64::try_from(sql_limit).unwrap_or(i64::MAX),
    ));
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare canonical Local recovery timeline root query")?;
    statement
        .query_map(params_from_iter(parameters), |row| {
            let mut root = map_root_without_change(row)?;
            root.changed_at_ms = Some(row.get(5)?);
            Ok(root)
        })
        .context("failed to query canonical Local recovery timeline roots")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode canonical Local recovery timeline roots")
}

fn roots_cte_for_query(query: &HistoryTimelineQuery) -> (String, Vec<SqlValue>) {
    if query.agent_id.is_none() && query.normalized_workdir.is_none() {
        return (ROOTS_CTE.to_string(), Vec::new());
    }

    // The public Liveboard requests history/recovery per Agent. Filtering only
    // after ROOTS_CTE forces SQLite to reconstruct every Agent's canonical
    // roots first, including the legacy orphan-poll grouping pass. On a large
    // history database that turns a 20-row focused page into a multi-second
    // global scan. Build the same three canonical root classes from rows that
    // can satisfy the representative filter instead. The orphan branch still
    // checks for a globally newer representative, so cross-Agent legacy groups
    // keep exactly the same ownership semantics as ROOTS_CTE.
    let mut parameters = Vec::<SqlValue>::new();
    let mut sql = r#"
WITH roots(root_id, representative_id, root_started_at_ms, orphan_kind, orphan_handle) AS (
    SELECT i.id, i.id, i.started_at_ms, 0, NULL
    FROM invocations i
    WHERE NOT (
        i.tool_name = 'write_stdin'
        AND i.continuation_kind = 'poll'
    )
"#
    .to_string();
    push_invocation_filters(&mut sql, &mut parameters, "i", query);
    sql.push_str(
        r#"

    UNION ALL

    SELECT
        (
            SELECT MIN(first.id)
            FROM invocations first
            WHERE first.tool_name = 'write_stdin'
              AND first.continuation_kind = 'poll'
              AND first.target_created_by_invocation_id IS NULL
              AND first.target_session_handle = p.target_session_handle
        ),
        p.id,
        (
            SELECT MIN(first.started_at_ms)
            FROM invocations first
            WHERE first.tool_name = 'write_stdin'
              AND first.continuation_kind = 'poll'
              AND first.target_created_by_invocation_id IS NULL
              AND first.target_session_handle = p.target_session_handle
        ),
        1,
        p.target_session_handle
    FROM invocations p
    WHERE p.tool_name = 'write_stdin'
      AND p.continuation_kind = 'poll'
      AND p.target_created_by_invocation_id IS NULL
      AND p.target_session_handle IS NOT NULL
"#,
    );
    push_invocation_filters(&mut sql, &mut parameters, "p", query);
    sql.push_str(
        r#"
      AND NOT EXISTS (
          SELECT 1
          FROM invocations newer
          WHERE newer.tool_name = 'write_stdin'
            AND newer.continuation_kind = 'poll'
            AND newer.target_created_by_invocation_id IS NULL
            AND newer.target_session_handle = p.target_session_handle
            AND (
                newer.started_at_ms > p.started_at_ms
                OR (
                    newer.started_at_ms = p.started_at_ms
                    AND newer.id > p.id
                )
            )
      )

    UNION ALL

    SELECT p.id, p.id, p.started_at_ms, 2, NULL
    FROM invocations p
    WHERE p.tool_name = 'write_stdin'
      AND p.continuation_kind = 'poll'
      AND p.target_created_by_invocation_id IS NULL
      AND p.target_session_handle IS NULL
"#,
    );
    push_invocation_filters(&mut sql, &mut parameters, "p", query);
    sql.push_str("\n)\n");
    (sql, parameters)
}

fn push_invocation_filters(
    sql: &mut String,
    parameters: &mut Vec<SqlValue>,
    alias: &str,
    query: &HistoryTimelineQuery,
) {
    if let Some(agent_id) = &query.agent_id {
        sql.push_str(&format!("      AND {alias}.agent_id = ?\n"));
        parameters.push(SqlValue::Text(agent_id.clone()));
    }
    if let Some(workdir) = &query.normalized_workdir {
        sql.push_str(&format!(
            "      AND {alias}.declared_workdir_normalized = ?\n"
        ));
        parameters.push(SqlValue::Text(workdir.clone()));
    }
}

fn map_root_without_change(row: &rusqlite::Row<'_>) -> rusqlite::Result<TimelineRoot> {
    Ok(TimelineRoot {
        root_id: row.get(0)?,
        representative_id: row.get(1)?,
        started_at_ms: row.get(2)?,
        orphan_kind: row.get(3)?,
        orphan_handle: row.get(4)?,
        changed_at_ms: None,
    })
}

fn root_cursor(query: &HistoryTimelineQuery, root: &TimelineRoot) -> HistoryTimelineCursor {
    match query.mode {
        HistoryTimelineMode::History { before_ms } => HistoryTimelineCursor::History {
            started_at_ms: root.started_at_ms,
            root_id: root.root_id,
            before_ms,
        },
        HistoryTimelineMode::Recovery { since_ms, .. } => HistoryTimelineCursor::Recovery {
            changed_at_ms: root
                .changed_at_ms
                .expect("recovery root must carry its effective change time"),
            root_id: root.root_id,
            since_ms,
        },
    }
}

fn hydrate_roots(
    connection: &Connection,
    roots: &[TimelineRoot],
    projection: HistoryDiffProjection,
) -> Result<Vec<PresentationRecord>> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let root_ids = roots.iter().map(|root| root.root_id).collect::<Vec<_>>();
    let mut materialized = load_materialized(connection, &root_ids, projection)?;
    if materialized.len() == roots.len() {
        return roots
            .iter()
            .map(|root| {
                materialized.remove(&root.root_id).with_context(|| {
                    format!("materialized Local timeline lost root {}", root.root_id)
                })
            })
            .collect();
    }
    let missing = roots
        .iter()
        .filter(|root| !materialized.contains_key(&root.root_id))
        .cloned()
        .collect::<Vec<_>>();
    let hydrated = hydrate_roots_uncached(connection, &missing)?;
    let mut hydrated_by_root = missing
        .iter()
        .map(|root| root.root_id)
        .zip(hydrated)
        .collect::<HashMap<_, _>>();
    roots
        .iter()
        .map(|root| {
            if let Some(record) = materialized.remove(&root.root_id) {
                return Ok(record);
            }
            let record = hydrated_by_root
                .remove(&root.root_id)
                .with_context(|| format!("hydrated Local timeline lost root {}", root.root_id))?;
            Ok(match projection {
                HistoryDiffProjection::Summary => {
                    crate::local::presentation::project_diff_lines(&record, false)
                }
                HistoryDiffProjection::Full => record,
            })
        })
        .collect()
}

fn hydrate_roots_uncached(
    connection: &Connection,
    roots: &[TimelineRoot],
) -> Result<Vec<PresentationRecord>> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let representative_ids = roots
        .iter()
        .map(|root| root.representative_id)
        .collect::<Vec<_>>();
    let mut inputs = load_lean_inputs(connection, &representative_ids)?;
    load_target_commands_bulk(connection, &mut inputs)?;
    load_file_evidence_bulk(connection, &representative_ids, &mut inputs)?;

    let command_root_ids = roots
        .iter()
        .filter_map(|root| {
            let input = inputs.get(&root.representative_id)?;
            (root.orphan_kind == 0 && input.tool_name == "exec_command").then_some(root.root_id)
        })
        .collect::<Vec<_>>();
    let parent_polls = load_parent_poll_aggregates(connection, &command_root_ids)?;
    let orphan_handles = roots
        .iter()
        .filter(|root| root.orphan_kind == 1)
        .filter_map(|root| root.orphan_handle.clone())
        .collect::<Vec<_>>();
    let handle_polls = load_handle_poll_aggregates(connection, &orphan_handles)?;

    let mut ordered_inputs = Vec::with_capacity(roots.len());
    for root in roots {
        let mut input = inputs
            .remove(&root.representative_id)
            .with_context(|| format!("timeline root {} lost its representative", root.root_id))?;
        match root.orphan_kind {
            0 => {
                if input.tool_name == "exec_command" {
                    input.folded_polls = parent_polls.get(&root.root_id).cloned();
                }
            }
            1 => {
                let handle = root
                    .orphan_handle
                    .as_ref()
                    .context("orphan timeline group is missing its session handle")?;
                let aggregate = handle_polls
                    .get(handle)
                    .cloned()
                    .context("orphan timeline group lost its poll aggregate")?;
                input.orphan_poll_group = Some(PresentationOrphanPollInput {
                    primary_invocation_id: root.root_id,
                    started_at_ms: root.started_at_ms,
                    aggregate,
                });
            }
            2 => {
                let aggregate = single_poll_aggregate(&input);
                input.orphan_poll_group = Some(PresentationOrphanPollInput {
                    primary_invocation_id: root.root_id,
                    started_at_ms: root.started_at_ms,
                    aggregate,
                });
            }
            other => bail!("unsupported canonical timeline orphan kind {other}"),
        }
        ordered_inputs.push(input);
    }

    Ok(build_presentation_inputs(&ordered_inputs, &[]).records)
}

pub(super) fn timeline_detail_uncached(
    connection: &Connection,
    presentation_root_id: i64,
) -> Result<Option<PresentationRecord>> {
    let sql = format!(
        "{ROOTS_CTE}
         SELECT root_id, representative_id, root_started_at_ms, orphan_kind, orphan_handle
         FROM roots WHERE root_id = ?1 LIMIT 1"
    );
    let root = connection
        .query_row(&sql, [presentation_root_id], map_root_without_change)
        .optional()
        .context("failed to query canonical Local timeline detail root")?;
    root.map(|root| timeline_detail_uncached_from_root(connection, root))
        .transpose()
}

fn timeline_detail_uncached_from_root(
    connection: &Connection,
    root: TimelineRoot,
) -> Result<PresentationRecord> {
    hydrate_roots_uncached(connection, &[root])?
        .pop()
        .context("canonical Local timeline detail root produced no presentation")
}

fn load_lean_inputs(
    connection: &Connection,
    invocation_ids: &[i64],
) -> Result<HashMap<i64, PresentationInput>> {
    if invocation_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = placeholders(invocation_ids.len());
    let sql = format!("{LEAN_TIMELINE_INVOCATION_SELECT} WHERE id IN ({placeholders})");
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare lean Local timeline invocation query")?;
    let parameters = invocation_ids
        .iter()
        .copied()
        .map(SqlValue::Integer)
        .collect::<Vec<_>>();
    let records = statement
        .query_map(params_from_iter(parameters), map_presentation_input)
        .context("failed to query lean Local timeline invocations")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode lean Local timeline invocations")?;
    Ok(records
        .into_iter()
        .map(|record| (record.id, record))
        .collect())
}

fn map_presentation_input(row: &rusqlite::Row<'_>) -> rusqlite::Result<PresentationInput> {
    let args_json: String = row.get(3)?;
    let arguments = serde_json::from_str(&args_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            args_json.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    Ok(PresentationInput {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        tool_name: row.get(2)?,
        arguments,
        declared_workdir_exact: row.get(4)?,
        declared_workdir_normalized: row.get(5)?,
        is_new_workdir: row.get::<_, i64>(6)? != 0,
        started_at_ms: row.get(7)?,
        completed_at_ms: row.get(8)?,
        duration_ms: row.get(9)?,
        outcome_kind: row.get(10)?,
        error: row.get(11)?,
        result_summary: None,
        result_output: None,
        evidence_state: row.get(12)?,
        evidence_reason: row.get(13)?,
        capture_state: row.get(14)?,
        capture_reason: row.get(15)?,
        target_session_handle: row.get(16)?,
        target_created_by_agent_id: row.get(17)?,
        target_created_by_invocation_id: row.get(18)?,
        target_command: None,
        continuation_kind: row.get(19)?,
        cross_agent: row.get::<_, Option<i64>>(20)?.map(|value| value != 0),
        result_status: row.get(21)?,
        result_cwd: row.get(22)?,
        result_session_handle: row.get(23)?,
        result_exit_code: row.get(24)?,
        result_termination_reason: row.get(25)?,
        process_state: row.get(26)?,
        process_started_at_ms: row.get(27)?,
        process_ended_at_ms: row.get(28)?,
        process_exit_code: row.get(29)?,
        process_termination_reason: row.get(30)?,
        process_cwd: row.get(31)?,
        process_incomplete_reason: row.get(32)?,
        output_preview: None,
        output_preview_truncated: false,
        file_evidence: Vec::new(),
        folded_polls: None,
        orphan_poll_group: None,
    })
}

fn load_target_commands_bulk(
    connection: &Connection,
    inputs: &mut HashMap<i64, PresentationInput>,
) -> Result<()> {
    let parent_ids = inputs
        .values()
        .filter(|input| input.continuation_kind.as_deref() == Some("kill"))
        .filter_map(|input| input.target_created_by_invocation_id)
        .collect::<HashSet<_>>();
    if parent_ids.is_empty() {
        return Ok(());
    }
    let parent_ids = parent_ids.into_iter().collect::<Vec<_>>();
    let sql = format!(
        "SELECT id, args_json FROM invocations WHERE id IN ({}) AND tool_name = 'exec_command'",
        placeholders(parent_ids.len())
    );
    let parameters = parent_ids
        .iter()
        .copied()
        .map(SqlValue::Integer)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare Local kill target-command query")?;
    let commands = statement
        .query_map(params_from_iter(parameters), |row| {
            let id: i64 = row.get(0)?;
            let args_json: String = row.get(1)?;
            let command = serde_json::from_str::<serde_json::Value>(&args_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("cmd")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                });
            Ok((id, command))
        })
        .context("failed to query Local kill target commands")?
        .collect::<rusqlite::Result<HashMap<_, _>>>()
        .context("failed to decode Local kill target commands")?;
    for input in inputs.values_mut() {
        if input.continuation_kind.as_deref() != Some("kill") {
            continue;
        }
        if let Some(parent_id) = input.target_created_by_invocation_id {
            input.target_command = commands.get(&parent_id).cloned().flatten();
        }
    }
    Ok(())
}

fn load_file_evidence_bulk(
    connection: &Connection,
    invocation_ids: &[i64],
    inputs: &mut HashMap<i64, PresentationInput>,
) -> Result<()> {
    if invocation_ids.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "SELECT invocation_id, ordinal, source_kind, operation_hint, path_before, path_after,
                before_state, before_text, before_reason,
                destination_before_state, destination_before_text, destination_before_reason,
                after_state, after_text, after_reason,
                source_after_state, source_after_text, source_after_reason
         FROM invocation_file_evidence
         WHERE invocation_id IN ({})
         ORDER BY invocation_id ASC, ordinal ASC",
        placeholders(invocation_ids.len())
    );
    let parameters = invocation_ids
        .iter()
        .copied()
        .map(SqlValue::Integer)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare bulk Local timeline file-evidence query")?;
    let mut rows = statement
        .query(params_from_iter(parameters))
        .context("failed to query bulk Local timeline file evidence")?;
    while let Some(row) = rows
        .next()
        .context("failed to iterate bulk Local timeline file evidence")?
    {
        let invocation_id: i64 = row.get(0)?;
        let evidence = map_file_evidence_at(row, 1)?;
        if let Some(input) = inputs.get_mut(&invocation_id) {
            input.file_evidence.push(evidence);
        }
    }
    Ok(())
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn single_poll_aggregate(input: &PresentationInput) -> PresentationPollAggregateInput {
    PresentationPollAggregateInput {
        count: 1,
        final_status: Some(status_from_input(input)),
        final_cwd: input.result_cwd.clone(),
        final_exit_code: input.result_exit_code,
        final_termination_reason: input.result_termination_reason.clone(),
        latest_completed_at_ms: input.completed_at_ms,
        caller_agent_ids: input.agent_id.clone().into_iter().collect(),
        cross_agent: input.cross_agent == Some(true),
        raw_invocation_ids: vec![input.id],
        evidence: evidence_from_input(input),
    }
}

fn status_from_input(input: &PresentationInput) -> String {
    input.result_status.clone().unwrap_or_else(|| {
        if input.outcome_kind.as_deref() == Some("error") {
            "failed".to_string()
        } else if input.completed_at_ms.is_none() {
            "in_progress".to_string()
        } else {
            "completed".to_string()
        }
    })
}

fn evidence_from_input(input: &PresentationInput) -> PresentationEvidence {
    PresentationEvidence {
        evidence_state: input.evidence_state.clone(),
        capture_state: input.capture_state.clone(),
        degraded: input.evidence_state == "incomplete" || input.capture_state == "incomplete",
        reason: input
            .evidence_reason
            .as_deref()
            .or(input.capture_reason.as_deref())
            .map(sanitize_display_text),
    }
}
