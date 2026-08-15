use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::schema::verify_readable_schema;
use super::store::{canonical_json, normalize_declared_workdir, physical_store_size};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_output: Option<String>,
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
        verify_readable_schema(&connection)?;

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
        if query.include_raw || query.invocation_id.is_some() {
            for record in &mut records {
                record.full_output = Some(load_full_output(&connection, record.id)?);
            }
        }
        Ok(records)
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
        full_output: None,
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
            .map(|value| format!(" · {value}"))
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
            let _ = writeln!(output, "  - arguments: `{}`", markdown_inline_escape(&args));
            if let Some(result) = &record.result {
                let result = canonical_json(result).unwrap_or_else(|_| "null".to_string());
                let _ = writeln!(output, "  - result: `{}`", markdown_inline_escape(&result));
            }
            if let Some(error) = &record.error {
                let escaped = serde_json::to_string(error).unwrap_or_else(|_| "null".to_string());
                let _ = writeln!(output, "  - error: `{}`", markdown_inline_escape(&escaped));
            }
            if let Some(full_output) = &record.full_output {
                let escaped =
                    serde_json::to_string(full_output).unwrap_or_else(|_| "null".to_string());
                let _ = writeln!(
                    output,
                    "  - full PTY text (evidence {}, capture {}): `{}`",
                    record.evidence_state,
                    record.capture_state,
                    markdown_inline_escape(&escaped)
                );
            }
        }
    }
    output
}

fn markdown_inline_escape(value: &str) -> String {
    value.replace('`', "\\`")
}
