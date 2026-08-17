use std::collections::HashMap;
use std::hash::Hash;

use anyhow::{Context, Result};
use rusqlite::Connection;
use rusqlite::params_from_iter;
use rusqlite::types::{FromSql, Value as SqlValue};

use crate::local::presentation::{
    PRESENTATION_RAW_INVOCATION_ID_SAMPLE_LIMIT, PresentationEvidence,
    PresentationPollAggregateInput, sanitize_display_text,
};

pub(super) fn load_parent_poll_aggregates(
    connection: &Connection,
    parent_ids: &[i64],
) -> Result<HashMap<i64, PresentationPollAggregateInput>> {
    if parent_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let filter = format!(
        "target_created_by_invocation_id IN ({})",
        placeholders(parent_ids.len())
    );
    load_poll_aggregates(
        connection,
        "target_created_by_invocation_id",
        &filter,
        parent_ids.iter().copied().map(SqlValue::Integer).collect(),
    )
}

pub(super) fn load_handle_poll_aggregates(
    connection: &Connection,
    handles: &[String],
) -> Result<HashMap<String, PresentationPollAggregateInput>> {
    if handles.is_empty() {
        return Ok(HashMap::new());
    }
    let filter = format!(
        "target_created_by_invocation_id IS NULL AND target_session_handle IN ({})",
        placeholders(handles.len())
    );
    load_poll_aggregates(
        connection,
        "target_session_handle",
        &filter,
        handles.iter().cloned().map(SqlValue::Text).collect(),
    )
}

fn load_poll_aggregates<K>(
    connection: &Connection,
    group_expression: &str,
    filter: &str,
    filter_parameters: Vec<SqlValue>,
) -> Result<HashMap<K, PresentationPollAggregateInput>>
where
    K: FromSql + Eq + Hash + Clone,
{
    let aggregate_sql = format!(
        "WITH matching AS (
             SELECT id, {group_expression} AS group_key, agent_id, started_at_ms,
                    completed_at_ms, outcome_kind, result_status, result_cwd,
                    result_exit_code, result_termination_reason,
                    evidence_state, evidence_reason, capture_state, capture_reason,
                    cross_agent
             FROM invocations
             WHERE tool_name = 'write_stdin' AND continuation_kind = 'poll' AND {filter}
         ),
         ranked AS (
             SELECT matching.*,
                    ROW_NUMBER() OVER (
                        PARTITION BY group_key ORDER BY started_at_ms DESC, id DESC
                    ) AS latest_rank
             FROM matching
         ),
         caller_first AS (
             SELECT group_key, agent_id, MIN(started_at_ms) AS first_seen_at_ms
             FROM matching
             WHERE agent_id IS NOT NULL
             GROUP BY group_key, agent_id
         ),
         caller_ranked AS (
             SELECT caller_first.*,
                    ROW_NUMBER() OVER (
                        PARTITION BY group_key ORDER BY first_seen_at_ms ASC, agent_id ASC
                    ) AS caller_rank
             FROM caller_first
         ),
         callers AS (
             SELECT group_key, GROUP_CONCAT(agent_id, ',') AS caller_ids
             FROM caller_ranked
             WHERE caller_rank <= 32
             GROUP BY group_key
         )
         SELECT ranked.group_key,
                COUNT(*),
                MAX(COALESCE(ranked.cross_agent, 0)),
                SUM(CASE WHEN ranked.evidence_state = 'incomplete' THEN 1 ELSE 0 END),
                SUM(CASE WHEN ranked.evidence_state = 'pending' THEN 1 ELSE 0 END),
                SUM(CASE WHEN ranked.capture_state = 'incomplete' THEN 1 ELSE 0 END),
                SUM(CASE WHEN ranked.capture_state = 'pending' THEN 1 ELSE 0 END),
                SUM(CASE WHEN ranked.capture_state = 'complete' THEN 1 ELSE 0 END),
                MAX(COALESCE(ranked.evidence_reason, ranked.capture_reason)),
                MAX(CASE WHEN ranked.latest_rank = 1 THEN ranked.result_status END),
                MAX(CASE WHEN ranked.latest_rank = 1 THEN ranked.outcome_kind END),
                MAX(CASE WHEN ranked.latest_rank = 1 THEN ranked.result_cwd END),
                MAX(CASE WHEN ranked.latest_rank = 1 THEN ranked.result_exit_code END),
                MAX(CASE WHEN ranked.latest_rank = 1 THEN ranked.result_termination_reason END),
                MAX(CASE WHEN ranked.latest_rank = 1 THEN ranked.completed_at_ms END),
                callers.caller_ids
         FROM ranked
         LEFT JOIN callers ON callers.group_key = ranked.group_key
         GROUP BY ranked.group_key, callers.caller_ids"
    );
    let mut statement = connection
        .prepare(&aggregate_sql)
        .context("failed to prepare lean Local timeline poll aggregate query")?;
    let rows = statement
        .query_map(params_from_iter(filter_parameters.clone()), |row| {
            let key: K = row.get(0)?;
            let count: i64 = row.get(1)?;
            let evidence_incomplete: i64 = row.get(3)?;
            let evidence_pending: i64 = row.get(4)?;
            let capture_incomplete: i64 = row.get(5)?;
            let capture_pending: i64 = row.get(6)?;
            let capture_complete: i64 = row.get(7)?;
            let reason: Option<String> = row.get(8)?;
            let result_status: Option<String> = row.get(9)?;
            let outcome_kind: Option<String> = row.get(10)?;
            let completed_at_ms: Option<i64> = row.get(14)?;
            let final_status = result_status.or_else(|| {
                Some(if outcome_kind.as_deref() == Some("error") {
                    "failed".to_string()
                } else if completed_at_ms.is_none() {
                    "in_progress".to_string()
                } else {
                    "completed".to_string()
                })
            });
            let evidence_state = if evidence_incomplete > 0 {
                "incomplete"
            } else if evidence_pending > 0 {
                "pending"
            } else {
                "complete"
            };
            let capture_state = if capture_incomplete > 0 {
                "incomplete"
            } else if capture_pending > 0 {
                "pending"
            } else if capture_complete > 0 {
                "complete"
            } else {
                "not_applicable"
            };
            let caller_ids: Option<String> = row.get(15)?;
            Ok((
                key,
                PresentationPollAggregateInput {
                    count: usize::try_from(count).unwrap_or(usize::MAX),
                    final_status,
                    final_cwd: row.get(11)?,
                    final_exit_code: row.get(12)?,
                    final_termination_reason: row.get(13)?,
                    latest_completed_at_ms: completed_at_ms,
                    caller_agent_ids: caller_ids
                        .as_deref()
                        .map(|value| value.split(',').map(str::to_owned).collect())
                        .unwrap_or_default(),
                    cross_agent: row.get::<_, i64>(2)? != 0,
                    raw_invocation_ids: Vec::new(),
                    evidence: PresentationEvidence {
                        evidence_state: evidence_state.to_string(),
                        capture_state: capture_state.to_string(),
                        degraded: evidence_incomplete > 0 || capture_incomplete > 0,
                        reason: reason.as_deref().map(sanitize_display_text),
                    },
                },
            ))
        })
        .context("failed to query lean Local timeline poll aggregates")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode lean Local timeline poll aggregates")?;
    let mut aggregates = rows.into_iter().collect::<HashMap<_, _>>();

    let sample_sql = format!(
        "WITH matching AS (
             SELECT id, {group_expression} AS group_key, started_at_ms
             FROM invocations
             WHERE tool_name = 'write_stdin' AND continuation_kind = 'poll' AND {filter}
         ),
         sampled AS (
             SELECT matching.*,
                    ROW_NUMBER() OVER (
                        PARTITION BY group_key ORDER BY started_at_ms ASC, id ASC
                    ) AS sample_rank
             FROM matching
         )
         SELECT group_key, id
         FROM sampled
         WHERE sample_rank <= ?
         ORDER BY group_key, sample_rank"
    );
    let mut sample_parameters = filter_parameters;
    sample_parameters.push(SqlValue::Integer(
        i64::try_from(PRESENTATION_RAW_INVOCATION_ID_SAMPLE_LIMIT).unwrap_or(i64::MAX),
    ));
    let mut sample_statement = connection
        .prepare(&sample_sql)
        .context("failed to prepare bounded Local timeline poll identity sample query")?;
    let samples = sample_statement
        .query_map(params_from_iter(sample_parameters), |row| {
            Ok((row.get::<_, K>(0)?, row.get::<_, i64>(1)?))
        })
        .context("failed to query bounded Local timeline poll identity samples")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode bounded Local timeline poll identity samples")?;
    for (key, invocation_id) in samples {
        if let Some(aggregate) = aggregates.get_mut(&key) {
            aggregate.raw_invocation_ids.push(invocation_id);
        }
    }
    Ok(aggregates)
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}
