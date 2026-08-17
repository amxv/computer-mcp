use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::local::history::{
    HistoryAgentRecord, HistoryOutputChunk, HistoryOutputMetadata, HistoryTimelineCheckpoint,
};
use crate::local::presentation::{
    PRESENTATION_SCHEMA_VERSION, PresentationDocument, PresentationRecord,
};
use crate::local::{HistoryAgentWorkdir, HistoryInvocation, HistoryStoreStatus};

pub const LOCAL_OBSERVABILITY_API_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiStatusDocument {
    pub schema_version: u32,
    pub api_version: u32,
    pub presentation_version: u32,
    pub runtime_id: String,
    pub history: HistoryStoreStatus,
    pub current_runtime_agent_count: usize,
    pub active_process_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiAgentList {
    pub schema_version: u32,
    pub runtime_id: String,
    pub agents: Vec<ApiAgent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiAgentDetail {
    pub schema_version: u32,
    pub runtime_id: String,
    pub agent: ApiAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiAgent {
    pub id: String,
    pub first_seen_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub seen_in_current_runtime: bool,
    pub active_process_count: u64,
    pub workdirs: Vec<HistoryAgentWorkdir>,
}

impl ApiAgent {
    pub(super) fn from_record(
        record: HistoryAgentRecord,
        runtime_id: &str,
        active_process_count: u64,
    ) -> Self {
        Self {
            id: record.summary.id,
            first_seen_at_ms: record.summary.first_seen_at_ms,
            last_seen_at_ms: record.summary.last_seen_at_ms,
            seen_in_current_runtime: record.last_seen_runtime_id == runtime_id,
            active_process_count,
            workdirs: record.summary.workdirs,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ApiInvocationList {
    pub schema_version: u32,
    pub presentation_version: u32,
    pub runtime_id: String,
    pub invocations: Vec<ApiLogicalInvocation>,
    pub presentation: PresentationDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ApiInvocationDetail {
    pub schema_version: u32,
    pub presentation_version: u32,
    pub runtime_id: String,
    pub invocation: ApiLogicalInvocation,
    pub presentation: PresentationDocument,
    pub output: HistoryOutputMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ApiLogicalInvocation {
    pub id: i64,
    pub correlation_id: String,
    pub agent_id: Option<String>,
    pub provider_kind: Option<String>,
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
}

impl From<HistoryInvocation> for ApiLogicalInvocation {
    fn from(record: HistoryInvocation) -> Self {
        Self {
            id: record.id,
            correlation_id: record.correlation_id,
            agent_id: record.agent_id,
            provider_kind: record.provider_kind,
            tool_name: record.tool_name,
            arguments: record.arguments,
            declared_workdir_exact: record.declared_workdir_exact,
            declared_workdir_normalized: record.declared_workdir_normalized,
            is_new_workdir: record.is_new_workdir,
            started_at_ms: record.started_at_ms,
            completed_at_ms: record.completed_at_ms,
            duration_ms: record.duration_ms,
            outcome_kind: record.outcome_kind,
            result: record.result,
            error: record.error,
            evidence_state: record.evidence_state,
            evidence_reason: record.evidence_reason,
            capture_state: record.capture_state,
            capture_reason: record.capture_reason,
            target_session_handle: record.target_session_handle,
            target_created_by_agent_id: record.target_created_by_agent_id,
            cross_agent: record.cross_agent,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiOutputPage {
    pub schema_version: u32,
    pub runtime_id: String,
    pub invocation_id: i64,
    pub view: String,
    pub chunks: Vec<HistoryOutputChunk>,
    pub next_cursor: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiTimelinePage {
    pub schema_version: u32,
    pub presentation_version: u32,
    pub runtime_id: String,
    pub records: Vec<PresentationRecord>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiTimelineDetail {
    pub schema_version: u32,
    pub presentation_version: u32,
    pub runtime_id: String,
    pub record: PresentationRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiTimelineCheckpointPage {
    pub schema_version: u32,
    pub presentation_version: u32,
    pub runtime_id: String,
    pub presentation_id: String,
    pub checkpoints: Vec<HistoryTimelineCheckpoint>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiError {
    pub schema_version: u32,
    pub error: String,
}

pub(super) fn schema_version() -> u32 {
    LOCAL_OBSERVABILITY_API_VERSION
}

pub(super) fn presentation_version() -> u32 {
    PRESENTATION_SCHEMA_VERSION
}
