use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::invocation::InvocationStart;

use super::diff::build_text_diff;
use super::model::{
    PRESENTATION_SCHEMA_VERSION, PresentationAgent, PresentationDocument, PresentationEvidence,
    PresentationFileChange, PresentationFileOperation, PresentationKind, PresentationPollSummary,
    PresentationRecord, PresentationWorkdir, PresentationWriteMode,
};
use super::sanitize::{sanitize_display_text, sanitize_preview};
use crate::local::file_evidence::parse_file_capture_plans;
use crate::local::{HistoryAgentSummary, HistoryFileEvidence, HistoryInvocation};

const MAX_COMMAND_PREVIEW_CHARS: usize = 4_096;
const MAX_OUTPUT_PREVIEW_CHARS: usize = 16_384;
const MAX_STDIN_PREVIEW_CHARS: usize = 4_096;
const MAX_GENERIC_SUMMARY_CHARS: usize = 2_048;

pub fn build_presentation(
    records: &[HistoryInvocation],
    agents: &[HistoryAgentSummary],
) -> PresentationDocument {
    let mut polls_by_handle = HashMap::<String, Vec<&HistoryInvocation>>::new();
    let mut continuations_by_handle = HashMap::<String, Vec<&HistoryInvocation>>::new();
    let mut parent_handles = HashSet::new();

    for record in records {
        if record.tool_name == "exec_command"
            && let Some(handle) = record.result_session_handle.as_ref()
        {
            parent_handles.insert(handle.clone());
        }
        if record.tool_name == "write_stdin"
            && let Some(handle) = record.target_session_handle.as_ref()
        {
            continuations_by_handle
                .entry(handle.clone())
                .or_default()
                .push(record);
            if is_poll(record) {
                polls_by_handle
                    .entry(handle.clone())
                    .or_default()
                    .push(record);
            }
        }
    }
    for values in polls_by_handle.values_mut() {
        values.sort_by_key(|record| (record.started_at_ms, record.id));
    }
    for values in continuations_by_handle.values_mut() {
        values.sort_by_key(|record| (record.started_at_ms, record.id));
    }

    let mut emitted_orphan_polls = HashSet::new();
    let mut presented = Vec::new();
    for record in records {
        if is_poll(record) {
            let Some(handle) = record.target_session_handle.as_ref() else {
                presented.push(generic_record(record));
                continue;
            };
            if parent_handles.contains(handle) {
                continue;
            }
            if emitted_orphan_polls.insert(handle.clone())
                && let Some(polls) = polls_by_handle.get(handle)
            {
                presented.push(orphan_poll_record(polls));
            }
            continue;
        }

        if let Some(changes) = build_file_changes(record) {
            presented.push(record_with_kind(
                record,
                vec![record.id],
                record.duration_ms,
                PresentationKind::FileChanges {
                    source_tool: record.tool_name.clone(),
                    changes,
                },
            ));
            continue;
        }

        match record.tool_name.as_str() {
            "exec_command" => {
                let polls = record
                    .result_session_handle
                    .as_ref()
                    .and_then(|handle| polls_by_handle.get(handle))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let continuations = record
                    .result_session_handle
                    .as_ref()
                    .and_then(|handle| continuations_by_handle.get(handle))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                presented.push(command_record(record, polls, continuations));
            }
            "write_stdin" => presented.push(write_stdin_record(record)),
            _ => presented.push(generic_record(record)),
        }
    }

    PresentationDocument {
        schema_version: PRESENTATION_SCHEMA_VERSION,
        agents: agents.iter().map(present_agent).collect(),
        records: presented,
    }
}

fn present_agent(agent: &HistoryAgentSummary) -> PresentationAgent {
    PresentationAgent {
        id: agent.id.clone(),
        first_seen_at_ms: agent.first_seen_at_ms,
        last_seen_at_ms: agent.last_seen_at_ms,
        workdirs: agent
            .workdirs
            .iter()
            .map(|workdir| PresentationWorkdir {
                normalized_workdir: sanitize_display_text(&workdir.normalized_workdir),
                first_seen_at_ms: workdir.first_seen_at_ms,
                last_seen_at_ms: workdir.last_seen_at_ms,
                first_invocation_id: workdir.first_invocation_id,
                last_invocation_id: workdir.last_invocation_id,
                retained_invocation_count: workdir.retained_invocation_count,
            })
            .collect(),
    }
}

fn command_record(
    record: &HistoryInvocation,
    polls: &[&HistoryInvocation],
    continuations: &[&HistoryInvocation],
) -> PresentationRecord {
    let command = record
        .arguments
        .get("cmd")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<unavailable command>");
    let (command, _) = sanitize_preview(command, MAX_COMMAND_PREVIEW_CHARS);
    let latest = continuations
        .iter()
        .rev()
        .copied()
        .find(|continuation| continuation.result_status.is_some())
        .unwrap_or(record);
    let output_source = record
        .output_preview
        .as_deref()
        .or_else(|| tool_output_text(latest))
        .or_else(|| tool_output_text(record));
    let (output, output_truncated) = output_source
        .map(|output| sanitize_preview(output, MAX_OUTPUT_PREVIEW_CHARS))
        .map(|(value, truncated)| (Some(value), truncated || record.output_preview_truncated))
        .unwrap_or((None, false));

    let poll_summary = (!polls.is_empty()).then(|| PresentationPollSummary {
        count: polls.len(),
        final_status: polls.last().map(|poll| status_for(poll)),
        caller_agent_ids: unique_agent_ids(polls),
        cross_agent: polls.iter().any(|poll| poll.cross_agent == Some(true)),
    });
    let mut raw_ids = vec![record.id];
    raw_ids.extend(
        continuations
            .iter()
            .filter(|continuation| is_poll(continuation) || continuation.result_status.is_some())
            .map(|continuation| continuation.id),
    );
    let duration = latest
        .completed_at_ms
        .map(|completed| completed.saturating_sub(record.started_at_ms))
        .or(record.duration_ms);

    let mut presented = record_with_kind(
        record,
        raw_ids,
        duration,
        PresentationKind::Command {
            command,
            status: status_for(latest),
            effective_cwd: latest
                .result_cwd
                .as_deref()
                .or(record.result_cwd.as_deref())
                .map(sanitize_display_text),
            exit_code: latest.result_exit_code.or(record.result_exit_code),
            termination_reason: latest
                .result_termination_reason
                .clone()
                .or_else(|| record.result_termination_reason.clone()),
            output,
            output_truncated,
            polls: poll_summary,
        },
    );
    if !continuations.is_empty() {
        let mut evidence_records = Vec::with_capacity(continuations.len() + 1);
        evidence_records.push(record);
        evidence_records.extend(
            continuations.iter().copied().filter(|continuation| {
                is_poll(continuation) || continuation.result_status.is_some()
            }),
        );
        presented.evidence = combined_evidence(&evidence_records);
    }
    presented
}

fn write_stdin_record(record: &HistoryInvocation) -> PresentationRecord {
    let handle = record
        .target_session_handle
        .as_deref()
        .unwrap_or("<unknown-session>");
    let kill = record
        .arguments
        .get("kill_process")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let kind = if kill {
        PresentationKind::Kill {
            target_session_handle: sanitize_display_text(handle),
            creator_agent_id: record.target_created_by_agent_id.clone(),
            cross_agent: record.cross_agent == Some(true),
            result_status: Some(status_for(record)),
        }
    } else if let Some(chars) = record
        .arguments
        .get("chars")
        .and_then(serde_json::Value::as_str)
    {
        let (chars, chars_truncated) = sanitize_preview(chars, MAX_STDIN_PREVIEW_CHARS);
        PresentationKind::Stdin {
            target_session_handle: sanitize_display_text(handle),
            chars,
            chars_truncated,
            creator_agent_id: record.target_created_by_agent_id.clone(),
            cross_agent: record.cross_agent == Some(true),
            result_status: Some(status_for(record)),
        }
    } else {
        return generic_record(record);
    };
    record_with_kind(record, vec![record.id], record.duration_ms, kind)
}

fn orphan_poll_record(polls: &[&HistoryInvocation]) -> PresentationRecord {
    let latest = polls.last().copied().expect("poll group is non-empty");
    let handle = latest
        .target_session_handle
        .as_deref()
        .unwrap_or("<unknown-session>");
    let mut record = record_with_kind(
        latest,
        polls.iter().map(|poll| poll.id).collect(),
        latest.duration_ms,
        PresentationKind::PollAggregate {
            target_session_handle: sanitize_display_text(handle),
            count: polls.len(),
            final_status: Some(status_for(latest)),
            creator_agent_id: latest.target_created_by_agent_id.clone(),
            caller_agent_ids: unique_agent_ids(polls),
            cross_agent: polls.iter().any(|poll| poll.cross_agent == Some(true)),
        },
    );
    record.evidence = combined_evidence(polls);
    record
}

fn generic_record(record: &HistoryInvocation) -> PresentationRecord {
    let summary = record
        .error
        .as_deref()
        .or_else(|| {
            record
                .result
                .as_ref()
                .and_then(serde_json::Value::as_object)
                .and_then(|result| result.get("summary"))
                .and_then(serde_json::Value::as_str)
        })
        .map(|summary| sanitize_preview(summary, MAX_GENERIC_SUMMARY_CHARS).0);
    record_with_kind(
        record,
        vec![record.id],
        record.duration_ms,
        PresentationKind::Generic {
            tool_name: sanitize_display_text(&record.tool_name),
            status: status_for(record),
            summary,
        },
    )
}

fn record_with_kind(
    record: &HistoryInvocation,
    raw_invocation_ids: Vec<i64>,
    duration_ms: Option<i64>,
    kind: PresentationKind,
) -> PresentationRecord {
    PresentationRecord {
        raw_invocation_ids,
        agent_id: record.agent_id.clone(),
        declared_workdir: record
            .declared_workdir_exact
            .as_deref()
            .map(sanitize_display_text),
        normalized_workdir: record
            .declared_workdir_normalized
            .as_deref()
            .map(sanitize_display_text),
        new_workdir: record.is_new_workdir.then(|| {
            sanitize_display_text(
                record
                    .declared_workdir_normalized
                    .as_deref()
                    .or(record.declared_workdir_exact.as_deref())
                    .unwrap_or("<unknown-workdir>"),
            )
        }),
        started_at_ms: record.started_at_ms,
        duration_ms,
        evidence: evidence_for(record),
        kind,
    }
}

fn evidence_for(record: &HistoryInvocation) -> PresentationEvidence {
    let degraded = record.evidence_state == "incomplete" || record.capture_state == "incomplete";
    let reason = record
        .evidence_reason
        .as_deref()
        .or(record.capture_reason.as_deref())
        .map(sanitize_display_text);
    PresentationEvidence {
        evidence_state: record.evidence_state.clone(),
        capture_state: record.capture_state.clone(),
        degraded,
        reason,
    }
}

fn combined_evidence(records: &[&HistoryInvocation]) -> PresentationEvidence {
    let degraded = records.iter().any(|record| evidence_for(record).degraded);
    let reason = records
        .iter()
        .find_map(|record| evidence_for(record).reason);
    let capture_state = if records
        .iter()
        .any(|record| record.capture_state == "incomplete")
    {
        "incomplete"
    } else if records
        .iter()
        .any(|record| record.capture_state == "pending")
    {
        "pending"
    } else if records
        .iter()
        .any(|record| record.capture_state == "complete")
    {
        "complete"
    } else {
        "not_applicable"
    };
    PresentationEvidence {
        evidence_state: if records
            .iter()
            .any(|record| record.evidence_state == "incomplete")
        {
            "incomplete"
        } else if records
            .iter()
            .any(|record| record.evidence_state == "pending")
        {
            "pending"
        } else {
            "complete"
        }
        .to_string(),
        capture_state: capture_state.to_string(),
        degraded,
        reason,
    }
}

fn status_for(record: &HistoryInvocation) -> String {
    record.result_status.clone().unwrap_or_else(|| {
        if record.outcome_kind.as_deref() == Some("error") {
            "failed".to_string()
        } else if record.completed_at_ms.is_none() {
            "in_progress".to_string()
        } else {
            "completed".to_string()
        }
    })
}

fn tool_output_text(record: &HistoryInvocation) -> Option<&str> {
    record.result.as_ref()?.as_object()?.get("output")?.as_str()
}

fn is_poll(record: &HistoryInvocation) -> bool {
    record.tool_name == "write_stdin"
        && record
            .arguments
            .get("chars")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|chars| chars.is_empty())
        && !record
            .arguments
            .get("kill_process")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
}

fn unique_agent_ids(records: &[&HistoryInvocation]) -> Vec<String> {
    let mut seen = HashSet::new();
    records
        .iter()
        .filter_map(|record| record.agent_id.clone())
        .filter(|agent_id| seen.insert(agent_id.clone()))
        .collect()
}

fn build_file_changes(record: &HistoryInvocation) -> Option<Vec<PresentationFileChange>> {
    if record.outcome_kind.as_deref() != Some("success") || record.file_evidence.is_empty() {
        return None;
    }
    if record.tool_name == "exec_command" && record.result_status.as_deref() != Some("exited") {
        return None;
    }

    let start = InvocationStart::new(record.tool_name.clone(), record.arguments.clone());
    let plans = parse_file_capture_plans(&start)?;
    if plans.len() != record.file_evidence.len() {
        return None;
    }

    plans
        .iter()
        .zip(&record.file_evidence)
        .map(|(plan, evidence)| build_file_change(plan, evidence))
        .collect()
}

fn build_file_change(
    plan: &crate::local::file_evidence::FileCapturePlan,
    evidence: &HistoryFileEvidence,
) -> Option<PresentationFileChange> {
    if evidence.source_kind != plan.source.as_str()
        || evidence.operation_hint != plan.operation.as_str()
        || Path::new(&evidence.path_before) != plan.path_before
        || Path::new(&evidence.path_after) != plan.path_after
    {
        return None;
    }

    let (operation, write_mode, before, after, old_path) = match evidence.operation_hint.as_str() {
        "create" => {
            if evidence.before_state != "missing" || evidence.after_state != "text" {
                return None;
            }
            (
                PresentationFileOperation::Created,
                None,
                "",
                evidence.after_text.as_deref()?,
                None,
            )
        }
        "update" => (
            PresentationFileOperation::Edited,
            None,
            text_state(&evidence.before_state, evidence.before_text.as_deref())?,
            text_state(&evidence.after_state, evidence.after_text.as_deref())?,
            None,
        ),
        "delete" => {
            if evidence.after_state != "missing" {
                return None;
            }
            (
                PresentationFileOperation::Deleted,
                None,
                text_state(&evidence.before_state, evidence.before_text.as_deref())?,
                "",
                None,
            )
        }
        "move" => {
            if evidence.destination_before_state.as_deref() != Some("missing")
                || evidence.source_after_state.as_deref() != Some("missing")
            {
                return None;
            }
            (
                PresentationFileOperation::Renamed,
                None,
                text_state(&evidence.before_state, evidence.before_text.as_deref())?,
                text_state(&evidence.after_state, evidence.after_text.as_deref())?,
                Some(sanitize_display_text(&evidence.path_before)),
            )
        }
        "overwrite" | "append" => {
            let before = missing_or_text(&evidence.before_state, evidence.before_text.as_deref())?;
            let after = text_state(&evidence.after_state, evidence.after_text.as_deref())?;
            if evidence.operation_hint == "append"
                && !before.is_empty()
                && !after.starts_with(before)
            {
                return None;
            }
            (
                if evidence.before_state == "missing" {
                    PresentationFileOperation::Created
                } else {
                    PresentationFileOperation::Edited
                },
                Some(if evidence.operation_hint == "append" {
                    PresentationWriteMode::Append
                } else {
                    PresentationWriteMode::Overwrite
                }),
                before,
                after,
                None,
            )
        }
        _ => return None,
    };
    let diff = build_text_diff(before, after)?;
    Some(PresentationFileChange {
        operation,
        path: sanitize_display_text(&evidence.path_after),
        old_path,
        write_mode,
        added: diff.added,
        removed: diff.removed,
        diff_truncated: diff.truncated,
        lines: diff.lines,
    })
}

fn text_state<'a>(state: &str, text: Option<&'a str>) -> Option<&'a str> {
    (state == "text").then_some(text).flatten()
}

fn missing_or_text<'a>(state: &str, text: Option<&'a str>) -> Option<&'a str> {
    match state {
        "missing" => Some(""),
        "text" => text,
        _ => None,
    }
}
