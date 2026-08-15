use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::local::presentation::{markdown_code_span, sanitize_display_text};

use super::schema::{readable_schema_version, verify_readable_schema};
use super::store::{canonical_json, normalize_declared_workdir, physical_store_size};

const PRESENTATION_OUTPUT_PREVIEW_CHARS: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryFormat {
    Markdown,
    Json,
}

impl HistoryFormat {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "markdown" => Ok(Self::Markdown),
            "json" => Ok(Self::Json),
            _ => bail!("history --format must be `markdown` or `json`"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HistoryQuery {
    pub last: usize,
    pub since_ms: Option<i64>,
    pub agent_id: Option<String>,
    pub normalized_workdir: Option<String>,
    pub invocation_id: Option<i64>,
    pub include_raw: bool,
}

impl HistoryQuery {
    pub fn recent(last: usize) -> Self {
        Self {
            last,
            ..Self::default()
        }
    }

    pub fn with_workdir(mut self, workdir: &Path) -> Result<Self> {
        if !workdir.is_absolute() {
            bail!("history --workdir must be an absolute path");
        }
        self.normalized_workdir = normalize_declared_workdir(&workdir.display().to_string());
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryInvocation {
    pub id: i64,
    pub correlation_id: String,
    pub agent_id: Option<String>,
    pub provider_kind: Option<String>,
    pub provider_session_key: Option<String>,
    pub tool_name: String,
    pub arguments: Value,
    pub declared_workdir_exact: Option<String>,
    pub declared_workdir_normalized: Option<String>,
    pub is_new_workdir: bool,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub outcome_kind: Option<String>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub evidence_state: String,
    pub evidence_reason: Option<String>,
    pub capture_state: String,
    pub capture_reason: Option<String>,
    pub target_session_handle: Option<String>,
    pub target_created_by_agent_id: Option<String>,
    pub cross_agent: Option<bool>,
    pub result_status: Option<String>,
    pub result_cwd: Option<String>,
    pub result_session_handle: Option<String>,
    pub result_exit_code: Option<i64>,
    pub result_termination_reason: Option<String>,
    #[serde(skip)]
    pub output_preview: Option<String>,
    #[serde(skip)]
    pub output_preview_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_output: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_evidence: Vec<HistoryFileEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryFileEvidence {
    pub ordinal: u32,
    pub source_kind: String,
    pub operation_hint: String,
    pub path_before: String,
    pub path_after: String,
    pub before_state: String,
    pub before_text: Option<String>,
    pub before_reason: Option<String>,
    pub destination_before_state: Option<String>,
    pub destination_before_text: Option<String>,
    pub destination_before_reason: Option<String>,
    pub after_state: String,
    pub after_text: Option<String>,
    pub after_reason: Option<String>,
    pub source_after_state: Option<String>,
    pub source_after_text: Option<String>,
    pub source_after_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryAgentWorkdir {
    pub normalized_workdir: String,
    pub ordinal: u32,
    pub first_seen_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub first_invocation_id: i64,
    pub last_invocation_id: i64,
    pub retained_invocation_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryAgentSummary {
    pub id: String,
    pub first_seen_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub workdirs: Vec<HistoryAgentWorkdir>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HistoryStoreStatus {
    pub database_exists: bool,
    pub physical_size_bytes: u64,
    pub health_state: String,
    pub health_reason: Option<String>,
    pub over_budget: bool,
    pub last_retention_error: Option<String>,
}

pub struct LocalHistoryReader;

impl LocalHistoryReader {
    pub fn query(path: &Path, query: &HistoryQuery) -> Result<Vec<HistoryInvocation>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let connection = open_read_only(path)?;
        let schema_version = readable_schema_version(&connection)?;

        let mut sql = String::from(
            "SELECT id, correlation_id, agent_id, provider_kind, provider_session_key,
                    tool_name, args_json, declared_workdir_exact, declared_workdir_normalized,
                    is_new_workdir, started_at_ms, completed_at_ms, duration_ms, outcome_kind,
                    result_json, error_text, evidence_state, evidence_reason, capture_state, capture_reason, target_session_handle,
                    target_created_by_agent_id, cross_agent, result_status, result_cwd,
                    result_session_handle, result_exit_code, result_termination_reason
             FROM invocations WHERE 1 = 1",
        );
        let mut parameters = Vec::<SqlValue>::new();
        if let Some(id) = query.invocation_id {
            sql.push_str(" AND id = ?");
            parameters.push(SqlValue::Integer(id));
        }
        if let Some(since_ms) = query.since_ms {
            sql.push_str(" AND started_at_ms >= ?");
            parameters.push(SqlValue::Integer(since_ms));
        }
        if let Some(agent_id) = &query.agent_id {
            sql.push_str(" AND agent_id = ?");
            parameters.push(SqlValue::Text(agent_id.clone()));
        }
        if let Some(workdir) = &query.normalized_workdir {
            sql.push_str(" AND declared_workdir_normalized = ?");
            parameters.push(SqlValue::Text(workdir.clone()));
        }
        sql.push_str(" ORDER BY started_at_ms DESC, id DESC LIMIT ?");
        parameters.push(SqlValue::Integer(
            i64::try_from(query.last.max(1)).unwrap_or(i64::MAX),
        ));

        let mut statement = connection
            .prepare(&sql)
            .context("failed to prepare Local history query")?;
        let mut records = statement
            .query_map(params_from_iter(parameters), map_invocation)
            .context("failed to query Local history")?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to decode Local history rows")?;
        for record in &mut records {
            if record.tool_name == "exec_command" {
                let (preview, truncated) =
                    load_output_preview(&connection, record.id, PRESENTATION_OUTPUT_PREVIEW_CHARS)?;
                record.output_preview = preview;
                record.output_preview_truncated = truncated;
            }
        }
        if query.include_raw {
            for record in &mut records {
                record.full_output = Some(load_full_output(&connection, record.id)?);
            }
        }
        if schema_version >= 3 {
            for record in &mut records {
                record.file_evidence = load_file_evidence(&connection, record.id)?;
            }
        }
        Ok(records)
    }

    pub fn agent_summaries(
        path: &Path,
        records: &[HistoryInvocation],
    ) -> Result<Vec<HistoryAgentSummary>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let connection = open_read_only(path)?;
        verify_readable_schema(&connection)?;
        let mut seen = HashSet::new();
        let mut agent_ids = Vec::new();
        for record in records.iter().rev() {
            if let Some(agent_id) = record.agent_id.as_ref()
                && seen.insert(agent_id.clone())
            {
                agent_ids.push(agent_id.clone());
            }
        }
        agent_ids
            .into_iter()
            .map(|agent_id| load_agent_summary(&connection, &agent_id))
            .collect()
    }

    pub fn status(path: &Path) -> Result<HistoryStoreStatus> {
        if !path.exists() {
            return Ok(HistoryStoreStatus {
                database_exists: false,
                physical_size_bytes: 0,
                health_state: "empty".to_string(),
                health_reason: None,
                over_budget: false,
                last_retention_error: None,
            });
        }
        let physical_size_bytes = physical_store_size(path)?;
        let unreadable = |reason: String| HistoryStoreStatus {
            database_exists: true,
            physical_size_bytes,
            health_state: "unreadable".to_string(),
            health_reason: Some(reason),
            over_budget: false,
            last_retention_error: None,
        };
        let connection = match open_read_only(path) {
            Ok(connection) => connection,
            Err(error) => return Ok(unreadable(error.to_string())),
        };
        if let Err(error) = verify_readable_schema(&connection) {
            return Ok(unreadable(error.to_string()));
        }
        let (health_state, health_reason, over_budget, last_retention_error) = match connection
            .query_row(
                "SELECT health_state, health_reason, over_budget, last_retention_error
                 FROM history_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, i64>(2)? != 0,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            ) {
            Ok(state) => state,
            Err(error) => {
                return Ok(unreadable(format!(
                    "failed to read Local history health state: {error}"
                )));
            }
        };
        Ok(HistoryStoreStatus {
            database_exists: true,
            physical_size_bytes,
            health_state,
            health_reason,
            over_budget,
            last_retention_error,
        })
    }

    pub fn render(
        records: &[HistoryInvocation],
        format: HistoryFormat,
        raw: bool,
    ) -> Result<String> {
        match format {
            HistoryFormat::Json => {
                serde_json::to_string_pretty(records).context("failed to render Local history JSON")
            }
            HistoryFormat::Markdown => Ok(render_markdown(records, raw)),
        }
    }
}

fn open_read_only(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| {
        format!(
            "failed to open Local history database {} read-only",
            path.display()
        )
    })
}

fn map_invocation(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryInvocation> {
    let args_json: String = row.get(6)?;
    let result_json: Option<String> = row.get(14)?;
    Ok(HistoryInvocation {
        id: row.get(0)?,
        correlation_id: row.get(1)?,
        agent_id: row.get(2)?,
        provider_kind: row.get(3)?,
        provider_session_key: row.get(4)?,
        tool_name: row.get(5)?,
        arguments: serde_json::from_str(&args_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                args_json.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        declared_workdir_exact: row.get(7)?,
        declared_workdir_normalized: row.get(8)?,
        is_new_workdir: row.get::<_, i64>(9)? != 0,
        started_at_ms: row.get(10)?,
        completed_at_ms: row.get(11)?,
        duration_ms: row.get(12)?,
        outcome_kind: row.get(13)?,
        result: result_json
            .map(|raw| {
                serde_json::from_str(&raw).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        raw.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .transpose()?,
        error: row.get(15)?,
        evidence_state: row.get(16)?,
        evidence_reason: row.get(17)?,
        capture_state: row.get(18)?,
        capture_reason: row.get(19)?,
        target_session_handle: row.get(20)?,
        target_created_by_agent_id: row.get(21)?,
        cross_agent: row.get::<_, Option<i64>>(22)?.map(|value| value != 0),
        result_status: row.get(23)?,
        result_cwd: row.get(24)?,
        result_session_handle: row.get(25)?,
        result_exit_code: row.get(26)?,
        result_termination_reason: row.get(27)?,
        output_preview: None,
        output_preview_truncated: false,
        full_output: None,
        file_evidence: Vec::new(),
    })
}

fn load_file_evidence(
    connection: &Connection,
    invocation_id: i64,
) -> Result<Vec<HistoryFileEvidence>> {
    let mut statement = connection
        .prepare(
            "SELECT ordinal, source_kind, operation_hint, path_before, path_after,
                    before_state, before_text, before_reason,
                    destination_before_state, destination_before_text, destination_before_reason,
                    after_state, after_text, after_reason, source_after_state, source_after_text, source_after_reason
             FROM invocation_file_evidence WHERE invocation_id = ?1 ORDER BY ordinal ASC",
        )
        .context("failed to prepare Local file-evidence query")?;
    statement
        .query_map([invocation_id], |row| {
            let ordinal: i64 = row.get(0)?;
            Ok(HistoryFileEvidence {
                ordinal: ordinal as u32,
                source_kind: row.get(1)?,
                operation_hint: row.get(2)?,
                path_before: row.get(3)?,
                path_after: row.get(4)?,
                before_state: row.get(5)?,
                before_text: row.get(6)?,
                before_reason: row.get(7)?,
                destination_before_state: row.get(8)?,
                destination_before_text: row.get(9)?,
                destination_before_reason: row.get(10)?,
                after_state: row.get(11)?,
                after_text: row.get(12)?,
                after_reason: row.get(13)?,
                source_after_state: row.get(14)?,
                source_after_text: row.get(15)?,
                source_after_reason: row.get(16)?,
            })
        })
        .context("failed to query Local file evidence")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode Local file evidence")
}

fn load_agent_summary(connection: &Connection, agent_id: &str) -> Result<HistoryAgentSummary> {
    let (first_seen_at_ms, last_seen_at_ms) = connection
        .query_row(
            "SELECT first_seen_at_ms, last_seen_at_ms FROM agents WHERE id = ?1",
            [agent_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .with_context(|| format!("failed to read Local Agent {agent_id}"))?;
    let mut statement = connection
        .prepare(
            "SELECT normalized_workdir, ordinal, first_seen_at_ms, last_seen_at_ms,
                    first_invocation_id, last_invocation_id, retained_invocation_count
             FROM agent_workdirs WHERE agent_id = ?1 ORDER BY ordinal ASC",
        )
        .context("failed to prepare Local Agent workdir query")?;
    let workdirs = statement
        .query_map([agent_id], |row| {
            let ordinal: i64 = row.get(1)?;
            let retained_invocation_count: i64 = row.get(6)?;
            Ok(HistoryAgentWorkdir {
                normalized_workdir: row.get(0)?,
                ordinal: ordinal as u32,
                first_seen_at_ms: row.get(2)?,
                last_seen_at_ms: row.get(3)?,
                first_invocation_id: row.get(4)?,
                last_invocation_id: row.get(5)?,
                retained_invocation_count: retained_invocation_count as u64,
            })
        })
        .context("failed to query Local Agent workdirs")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode Local Agent workdirs")?;
    Ok(HistoryAgentSummary {
        id: agent_id.to_string(),
        first_seen_at_ms,
        last_seen_at_ms,
        workdirs,
    })
}

fn load_full_output(connection: &Connection, invocation_id: i64) -> Result<String> {
    let mut statement = connection
        .prepare(
            "SELECT text FROM invocation_output_chunks
             WHERE invocation_id = ?1 ORDER BY sequence ASC",
        )
        .context("failed to prepare Local full-output query")?;
    let chunks = statement
        .query_map([invocation_id], |row| row.get::<_, String>(0))
        .context("failed to query Local full-output chunks")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode Local full-output chunks")?;
    Ok(chunks.concat())
}

fn load_output_preview(
    connection: &Connection,
    invocation_id: i64,
    max_chars: usize,
) -> Result<(Option<String>, bool)> {
    let mut statement = connection
        .prepare(
            "SELECT text FROM invocation_output_chunks
             WHERE invocation_id = ?1 ORDER BY sequence ASC",
        )
        .context("failed to prepare Local output-preview query")?;
    let mut rows = statement
        .query([invocation_id])
        .context("failed to query Local output-preview chunks")?;
    let mut preview = String::new();
    let mut chars_seen = 0usize;
    let mut saw_chunk = false;
    let preview_limit = max_chars.saturating_add(1);

    'chunks: while let Some(row) = rows
        .next()
        .context("failed to iterate Local output-preview chunks")?
    {
        saw_chunk = true;
        let chunk: String = row
            .get(0)
            .context("failed to decode Local output-preview chunk")?;
        for ch in chunk.chars() {
            if chars_seen >= preview_limit {
                break 'chunks;
            }
            preview.push(ch);
            chars_seen = chars_seen.saturating_add(1);
        }
    }

    if !saw_chunk {
        return Ok((None, false));
    }
    if chars_seen <= max_chars {
        return Ok((Some(preview), false));
    }

    let cut = preview
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(preview.len());
    preview.truncate(cut);
    Ok((Some(preview), true))
}

fn render_markdown(records: &[HistoryInvocation], raw: bool) -> String {
    if records.is_empty() {
        return "No Local history.\n".to_string();
    }
    let mut output = String::new();
    for record in records {
        let agent = record.agent_id.as_deref().unwrap_or("----");
        let outcome = record.outcome_kind.as_deref().unwrap_or("in-progress");
        let workdir = record
            .declared_workdir_normalized
            .as_deref()
            .map(sanitize_display_text)
            .map(|value| format!(" · {}", markdown_code_span(&value)))
            .unwrap_or_default();
        let cross_agent = if record.cross_agent == Some(true) {
            " · cross-agent"
        } else {
            ""
        };
        let _ = writeln!(
            output,
            "- `#{}` `{agent}` **{}** — {outcome}{workdir}{cross_agent}",
            record.id, record.tool_name
        );
        if raw {
            let args = canonical_json(&record.arguments).unwrap_or_else(|_| "null".to_string());
            let args = sanitize_display_text(&args);
            let _ = writeln!(output, "  - arguments: {}", markdown_code_span(&args));
            if let Some(result) = &record.result {
                let result = canonical_json(result).unwrap_or_else(|_| "null".to_string());
                let result = sanitize_display_text(&result);
                let _ = writeln!(output, "  - result: {}", markdown_code_span(&result));
            }
            if let Some(error) = &record.error {
                let escaped = serde_json::to_string(error).unwrap_or_else(|_| "null".to_string());
                let escaped = sanitize_display_text(&escaped);
                let _ = writeln!(output, "  - error: {}", markdown_code_span(&escaped));
            }
            if let Some(full_output) = &record.full_output {
                let escaped =
                    serde_json::to_string(full_output).unwrap_or_else(|_| "null".to_string());
                let escaped = sanitize_display_text(&escaped);
                let _ = writeln!(
                    output,
                    "  - full PTY text (evidence {}, capture {}): {}",
                    record.evidence_state,
                    record.capture_state,
                    markdown_code_span(&escaped)
                );
            }
        }
    }
    output
}
