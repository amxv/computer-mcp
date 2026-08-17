use std::path::PathBuf;

use serde_json::{Value, json};

use super::super::history::HistoryOutputMetadata;
use super::super::observability::{
    ApiAgent, ApiInvocationDetail, ApiLogicalInvocation, ApiStatusDocument,
};
use super::super::{
    HistoryAgentWorkdir, HistoryStoreStatus, LOCAL_DISCOVERY_SCHEMA_VERSION,
    LOCAL_OBSERVABILITY_API_VERSION, LocalObservabilityDiscovery, LocalRuntimeDiscovery,
    PRESENTATION_SCHEMA_VERSION, PresentationDocument, PresentationEvidence, PresentationKind,
    PresentationRecord,
};
use super::client::WatchBootstrap;

pub(super) const RUNTIME_ID: &str = "runtime-test-1234";

pub(super) fn agent(id: &str, workdirs: &[&str]) -> ApiAgent {
    ApiAgent {
        id: id.to_owned(),
        first_seen_at_ms: 1_000,
        last_seen_at_ms: 2_000,
        seen_in_current_runtime: true,
        active_process_count: 1,
        workdirs: workdirs
            .iter()
            .enumerate()
            .map(|(index, path)| HistoryAgentWorkdir {
                normalized_workdir: (*path).to_owned(),
                ordinal: index as u32,
                first_seen_at_ms: 1_000 + index as i64,
                last_seen_at_ms: 2_000,
                first_invocation_id: 1,
                last_invocation_id: 1,
                retained_invocation_count: 1,
            })
            .collect(),
    }
}

pub(super) fn bootstrap(agents: Vec<ApiAgent>) -> WatchBootstrap {
    WatchBootstrap {
        discovery: LocalRuntimeDiscovery {
            schema_version: LOCAL_DISCOVERY_SCHEMA_VERSION,
            runtime_id: RUNTIME_ID.to_owned(),
            pid: 123,
            start_directory: PathBuf::from("/workspace"),
            started_at: "2026-08-16T00:00:00Z".to_owned(),
            expires_at: Some("2026-08-17T00:00:00Z".to_owned()),
            observability: LocalObservabilityDiscovery::active(
                "http://127.0.0.1:43123",
                "/tmp/zodex-watch-test-bearer",
            ),
        },
        status: ApiStatusDocument {
            schema_version: LOCAL_OBSERVABILITY_API_VERSION,
            api_version: LOCAL_OBSERVABILITY_API_VERSION,
            presentation_version: PRESENTATION_SCHEMA_VERSION,
            runtime_id: RUNTIME_ID.to_owned(),
            history: HistoryStoreStatus {
                database_exists: true,
                physical_size_bytes: 1_024,
                health_state: "healthy".to_owned(),
                health_reason: None,
                over_budget: false,
                last_retention_error: None,
            },
            current_runtime_agent_count: agents.len(),
            active_process_count: agents.iter().map(|agent| agent.active_process_count).sum(),
        },
        agents,
    }
}

pub(super) fn detail(
    id: i64,
    agent_id: Option<&str>,
    tool_name: &str,
    arguments: Value,
    record: PresentationRecord,
) -> ApiInvocationDetail {
    ApiInvocationDetail {
        schema_version: LOCAL_OBSERVABILITY_API_VERSION,
        presentation_version: PRESENTATION_SCHEMA_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        invocation: ApiLogicalInvocation {
            id,
            correlation_id: format!("corr-{id}"),
            agent_id: agent_id.map(str::to_owned),
            provider_kind: Some("chatgpt".to_owned()),
            tool_name: tool_name.to_owned(),
            arguments,
            declared_workdir_exact: Some("/workspace".to_owned()),
            declared_workdir_normalized: Some("/workspace".to_owned()),
            is_new_workdir: false,
            started_at_ms: id * 1_000,
            completed_at_ms: None,
            duration_ms: None,
            outcome_kind: None,
            result: None,
            error: None,
            evidence_state: "complete".to_owned(),
            evidence_reason: None,
            capture_state: "complete".to_owned(),
            capture_reason: None,
            target_session_handle: None,
            target_created_by_agent_id: None,
            cross_agent: Some(false),
        },
        presentation: PresentationDocument {
            schema_version: PRESENTATION_SCHEMA_VERSION,
            agents: Vec::new(),
            records: vec![record],
        },
        output: HistoryOutputMetadata {
            available: false,
            chunk_count: 0,
            size_bytes: 0,
            capture_state: "complete".to_owned(),
            capture_reason: None,
            first_cursor: None,
            last_cursor: None,
        },
    }
}

pub(super) fn command_detail(
    id: i64,
    agent_id: Option<&str>,
    command: &str,
    status: &str,
    output: Option<&str>,
) -> ApiInvocationDetail {
    detail(
        id,
        agent_id,
        "exec_command",
        json!({"cmd": command, "yield_time_ms": 1000}),
        record(
            id,
            agent_id,
            PresentationKind::Command {
                command: command.to_owned(),
                status: status.to_owned(),
                effective_cwd: Some("/workspace/subdir".to_owned()),
                exit_code: (status == "success").then_some(0),
                termination_reason: None,
                output: output.map(str::to_owned),
                output_truncated: false,
                polls: None,
            },
        ),
    )
}

pub(super) fn poll_detail(id: i64, agent_id: &str, handle: &str) -> ApiInvocationDetail {
    detail(
        id,
        Some(agent_id),
        "write_stdin",
        json!({"session_handle": handle, "yield_time_ms": 1000}),
        record(
            id,
            Some(agent_id),
            PresentationKind::PollAggregate {
                target_session_handle: handle.to_owned(),
                count: 1,
                final_status: Some("running".to_owned()),
                creator_agent_id: Some(agent_id.to_owned()),
                caller_agent_ids: vec![agent_id.to_owned()],
                cross_agent: false,
            },
        ),
    )
}

pub(super) fn record(
    id: i64,
    agent_id: Option<&str>,
    kind: PresentationKind,
) -> PresentationRecord {
    PresentationRecord {
        presentation_id: format!("inv-{id}"),
        primary_invocation_id: id,
        raw_evidence_count: 1,
        raw_invocation_ids: vec![id],
        raw_invocation_ids_truncated: false,
        agent_id: agent_id.map(str::to_owned),
        declared_workdir: Some("/workspace".to_owned()),
        normalized_workdir: Some("/workspace".to_owned()),
        new_workdir: None,
        started_at_ms: id * 1_000,
        duration_ms: None,
        evidence: PresentationEvidence {
            evidence_state: "complete".to_owned(),
            capture_state: "complete".to_owned(),
            degraded: false,
            reason: None,
        },
        kind,
    }
}
