use serde_json::Value;

use crate::local::{HistoryFileEvidence, HistoryInvocation};

/// Purpose-specific input to canonical presentation normalization.
///
/// The current raw-history path adapts `HistoryInvocation` into this shape.
/// Timeline queries can construct it directly without loading exact cumulative
/// result bodies merely to render collapsed canonical cards.
#[derive(Debug, Clone)]
pub(crate) struct PresentationInput {
    pub id: i64,
    pub agent_id: Option<String>,
    pub tool_name: String,
    pub arguments: Value,
    pub declared_workdir_exact: Option<String>,
    pub declared_workdir_normalized: Option<String>,
    pub is_new_workdir: bool,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub outcome_kind: Option<String>,
    pub error: Option<String>,
    pub result_summary: Option<String>,
    pub result_output: Option<String>,
    pub evidence_state: String,
    pub evidence_reason: Option<String>,
    pub capture_state: String,
    pub capture_reason: Option<String>,
    pub target_session_handle: Option<String>,
    pub target_created_by_agent_id: Option<String>,
    pub target_created_by_invocation_id: Option<i64>,
    pub continuation_kind: Option<String>,
    pub cross_agent: Option<bool>,
    pub result_status: Option<String>,
    pub result_cwd: Option<String>,
    pub result_session_handle: Option<String>,
    pub result_exit_code: Option<i64>,
    pub result_termination_reason: Option<String>,
    pub process_state: Option<String>,
    pub process_started_at_ms: Option<i64>,
    pub process_ended_at_ms: Option<i64>,
    pub process_exit_code: Option<i64>,
    pub process_termination_reason: Option<String>,
    pub process_cwd: Option<String>,
    pub process_incomplete_reason: Option<String>,
    pub output_preview: Option<String>,
    pub output_preview_truncated: bool,
    pub file_evidence: Vec<HistoryFileEvidence>,
}

impl From<&HistoryInvocation> for PresentationInput {
    fn from(record: &HistoryInvocation) -> Self {
        let result_object = record.result.as_ref().and_then(Value::as_object);
        Self {
            id: record.id,
            agent_id: record.agent_id.clone(),
            tool_name: record.tool_name.clone(),
            arguments: record.arguments.clone(),
            declared_workdir_exact: record.declared_workdir_exact.clone(),
            declared_workdir_normalized: record.declared_workdir_normalized.clone(),
            is_new_workdir: record.is_new_workdir,
            started_at_ms: record.started_at_ms,
            completed_at_ms: record.completed_at_ms,
            duration_ms: record.duration_ms,
            outcome_kind: record.outcome_kind.clone(),
            error: record.error.clone(),
            result_summary: result_object
                .and_then(|result| result.get("summary"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            result_output: (record.tool_name != "write_stdin")
                .then(|| {
                    result_object
                        .and_then(|result| result.get("output"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .flatten(),
            evidence_state: record.evidence_state.clone(),
            evidence_reason: record.evidence_reason.clone(),
            capture_state: record.capture_state.clone(),
            capture_reason: record.capture_reason.clone(),
            target_session_handle: record.target_session_handle.clone(),
            target_created_by_agent_id: record.target_created_by_agent_id.clone(),
            target_created_by_invocation_id: record.target_created_by_invocation_id,
            continuation_kind: record.continuation_kind.clone(),
            cross_agent: record.cross_agent,
            result_status: record.result_status.clone(),
            result_cwd: record.result_cwd.clone(),
            result_session_handle: record.result_session_handle.clone(),
            result_exit_code: record.result_exit_code,
            result_termination_reason: record.result_termination_reason.clone(),
            process_state: record.process_state.clone(),
            process_started_at_ms: record.process_started_at_ms,
            process_ended_at_ms: record.process_ended_at_ms,
            process_exit_code: record.process_exit_code,
            process_termination_reason: record.process_termination_reason.clone(),
            process_cwd: record.process_cwd.clone(),
            process_incomplete_reason: record.process_incomplete_reason.clone(),
            output_preview: record.output_preview.clone(),
            output_preview_truncated: record.output_preview_truncated,
            file_evidence: record.file_evidence.clone(),
        }
    }
}
