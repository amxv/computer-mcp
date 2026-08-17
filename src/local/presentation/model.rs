use serde::{Deserialize, Serialize};

pub const PRESENTATION_SCHEMA_VERSION: u32 = 2;
pub(crate) const PRESENTATION_RAW_INVOCATION_ID_SAMPLE_LIMIT: usize = 32;

pub(crate) fn presentation_id_for_root(primary_invocation_id: i64) -> String {
    format!("inv-{primary_invocation_id}")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationDocument {
    pub schema_version: u32,
    pub agents: Vec<PresentationAgent>,
    pub records: Vec<PresentationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationAgent {
    pub id: String,
    pub first_seen_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub workdirs: Vec<PresentationWorkdir>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationWorkdir {
    pub normalized_workdir: String,
    pub first_seen_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub first_invocation_id: i64,
    pub last_invocation_id: i64,
    pub retained_invocation_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationRecord {
    pub presentation_id: String,
    pub primary_invocation_id: i64,
    pub raw_evidence_count: usize,
    pub raw_invocation_ids: Vec<i64>,
    pub raw_invocation_ids_truncated: bool,
    pub agent_id: Option<String>,
    pub declared_workdir: Option<String>,
    pub normalized_workdir: Option<String>,
    pub new_workdir: Option<String>,
    pub started_at_ms: i64,
    pub duration_ms: Option<i64>,
    pub evidence: PresentationEvidence,
    #[serde(flatten)]
    pub kind: PresentationKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationEvidence {
    pub evidence_state: String,
    pub capture_state: String,
    pub degraded: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PresentationKind {
    Command {
        command: String,
        status: String,
        effective_cwd: Option<String>,
        exit_code: Option<i64>,
        termination_reason: Option<String>,
        output: Option<String>,
        output_truncated: bool,
        polls: Option<PresentationPollSummary>,
    },
    FileChanges {
        source_tool: String,
        changes: Vec<PresentationFileChange>,
    },
    Stdin {
        target_session_handle: String,
        chars: String,
        chars_truncated: bool,
        creator_agent_id: Option<String>,
        cross_agent: bool,
        result_status: Option<String>,
    },
    Kill {
        target_session_handle: String,
        creator_agent_id: Option<String>,
        cross_agent: bool,
        result_status: Option<String>,
    },
    PollAggregate {
        target_session_handle: String,
        count: usize,
        final_status: Option<String>,
        creator_agent_id: Option<String>,
        caller_agent_ids: Vec<String>,
        cross_agent: bool,
    },
    Generic {
        tool_name: String,
        status: String,
        summary: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationPollSummary {
    pub count: usize,
    pub final_status: Option<String>,
    pub caller_agent_ids: Vec<String>,
    pub cross_agent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationFileChange {
    pub operation: PresentationFileOperation,
    pub path: String,
    pub old_path: Option<String>,
    pub write_mode: Option<PresentationWriteMode>,
    pub added: usize,
    pub removed: usize,
    pub diff_truncated: bool,
    pub lines: Vec<PresentationDiffLine>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PresentationFileOperation {
    Created,
    Edited,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PresentationWriteMode {
    Overwrite,
    Append,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationDiffLine {
    pub kind: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub text: String,
}
