use std::collections::{HashMap, HashSet};
use std::iter;
use std::path::Path;

use crate::invocation::InvocationStart;

use super::diff::build_text_diff;
use super::model::{
    PRESENTATION_RAW_INVOCATION_ID_SAMPLE_LIMIT, PRESENTATION_SCHEMA_VERSION, PresentationAgent,
    PresentationDocument, PresentationEvidence, PresentationFileChange, PresentationFileOperation,
    PresentationKind, PresentationPollSummary, PresentationRecord, PresentationWorkdir,
    PresentationWriteMode, presentation_id_for_root,
};
use super::sanitize::{sanitize_display_text, sanitize_preview};
use super::{PresentationInput, PresentationOrphanPollInput, PresentationPollAggregateInput};
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
    let inputs = records
        .iter()
        .map(PresentationInput::from)
        .collect::<Vec<_>>();
    build_presentation_inputs(&inputs, agents)
}

pub(crate) fn build_presentation_inputs(
    records: &[PresentationInput],
    agents: &[HistoryAgentSummary],
) -> PresentationDocument {
    let command_ids = records
        .iter()
        .filter(|record| record.tool_name == "exec_command")
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    let parent_handles = records
        .iter()
        .filter(|record| record.tool_name == "exec_command")
        .filter_map(|record| record.result_session_handle.clone())
        .collect::<HashSet<_>>();
    let mut polls_by_parent = HashMap::<i64, Vec<&PresentationInput>>::new();
    let mut legacy_polls_by_handle = HashMap::<String, Vec<&PresentationInput>>::new();

    for record in records.iter().filter(|record| is_poll(record)) {
        if let Some(parent_id) = record.target_created_by_invocation_id {
            polls_by_parent.entry(parent_id).or_default().push(record);
        } else if let Some(handle) = record.target_session_handle.as_ref() {
            legacy_polls_by_handle
                .entry(handle.clone())
                .or_default()
                .push(record);
        }
    }
    for values in polls_by_parent.values_mut() {
        sort_invocations(values);
    }
    for values in legacy_polls_by_handle.values_mut() {
        sort_invocations(values);
    }

    let mut emitted_parent_orphans = HashSet::new();
    let mut emitted_legacy_orphans = HashSet::new();
    let mut presented = Vec::new();
    for record in records {
        if let Some(orphan) = record.orphan_poll_group.as_ref() {
            presented.push(orphan_poll_aggregate_record(record, orphan));
            continue;
        }
        if is_poll(record) {
            if let Some(parent_id) = record.target_created_by_invocation_id {
                if command_ids.contains(&parent_id) {
                    continue;
                }
                if emitted_parent_orphans.insert(parent_id)
                    && let Some(polls) = polls_by_parent.get(&parent_id)
                {
                    presented.push(orphan_poll_record(polls, Some(parent_id)));
                }
                continue;
            }

            let Some(handle) = record.target_session_handle.as_ref() else {
                presented.push(generic_record(record));
                continue;
            };
            if parent_handles.contains(handle) {
                continue;
            }
            if emitted_legacy_orphans.insert(handle.clone())
                && let Some(polls) = legacy_polls_by_handle.get(handle)
            {
                presented.push(orphan_poll_record(polls, None));
            }
            continue;
        }

        if let Some(changes) = build_file_changes(record) {
            presented.push(record_with_kind(
                record,
                raw_identity(record.id, 1, iter::once(record.id)),
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
                let mut polls = polls_by_parent.get(&record.id).cloned().unwrap_or_default();
                if let Some(handle) = record.result_session_handle.as_ref()
                    && let Some(legacy) = legacy_polls_by_handle.get(handle)
                {
                    polls.extend(legacy.iter().copied());
                }
                sort_invocations(&mut polls);
                polls.dedup_by_key(|poll| poll.id);
                presented.push(command_record(record, &polls));
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

fn sort_invocations(records: &mut Vec<&PresentationInput>) {
    records.sort_by_key(|record| (record.started_at_ms, record.id));
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

#[derive(Debug)]
struct CommandFacts {
    status: String,
    effective_cwd: Option<String>,
    exit_code: Option<i64>,
    termination_reason: Option<String>,
    duration_ms: Option<i64>,
}

fn command_record(record: &PresentationInput, polls: &[&PresentationInput]) -> PresentationRecord {
    let command = record
        .arguments
        .get("cmd")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<unavailable command>");
    let (command, _) = sanitize_preview(command, MAX_COMMAND_PREVIEW_CHARS);
    let latest_poll = polls.last().copied();
    let facts = command_facts(record, latest_poll, record.folded_polls.as_ref());
    let output_source = record
        .output_preview
        .as_deref()
        .or(record.result_output.as_deref());
    let (output, output_truncated) = output_source
        .map(|output| sanitize_preview(output, MAX_OUTPUT_PREVIEW_CHARS))
        .map(|(value, truncated)| (Some(value), truncated || record.output_preview_truncated))
        .unwrap_or((None, false));

    let (poll_summary, identity, poll_evidence) = if let Some(aggregate) = &record.folded_polls {
        (
            (aggregate.count > 0).then(|| poll_summary_from_aggregate(aggregate)),
            raw_identity(
                record.id,
                aggregate.count.saturating_add(1),
                iter::once(record.id).chain(aggregate.raw_invocation_ids.iter().copied()),
            ),
            (aggregate.count > 0).then_some(&aggregate.evidence),
        )
    } else {
        (
            (!polls.is_empty()).then(|| PresentationPollSummary {
                count: polls.len(),
                final_status: latest_poll.map(status_for),
                caller_agent_ids: unique_agent_ids(polls),
                cross_agent: polls.iter().any(|poll| poll.cross_agent == Some(true)),
            }),
            raw_identity(
                record.id,
                polls.len().saturating_add(1),
                iter::once(record.id).chain(polls.iter().map(|poll| poll.id)),
            ),
            None,
        )
    };

    let mut presented = record_with_kind(
        record,
        identity,
        facts.duration_ms,
        PresentationKind::Command {
            command,
            status: facts.status,
            effective_cwd: facts.effective_cwd,
            exit_code: facts.exit_code,
            termination_reason: facts.termination_reason,
            output,
            output_truncated,
            polls: poll_summary,
        },
    );
    if let Some(poll_evidence) = poll_evidence {
        presented.evidence = combine_evidence_values(&presented.evidence, poll_evidence);
    } else if !polls.is_empty() {
        let mut evidence_records = Vec::with_capacity(polls.len() + 1);
        evidence_records.push(record);
        evidence_records.extend(polls.iter().copied());
        presented.evidence = combined_evidence(&evidence_records);
    }
    presented
}

fn poll_summary_from_aggregate(
    aggregate: &PresentationPollAggregateInput,
) -> PresentationPollSummary {
    PresentationPollSummary {
        count: aggregate.count,
        final_status: aggregate.final_status.clone(),
        caller_agent_ids: aggregate.caller_agent_ids.clone(),
        cross_agent: aggregate.cross_agent,
    }
}

fn command_facts(
    record: &PresentationInput,
    latest_poll: Option<&PresentationInput>,
    aggregate: Option<&PresentationPollAggregateInput>,
) -> CommandFacts {
    let lifecycle_cwd = record
        .process_cwd
        .as_deref()
        .or(record.result_cwd.as_deref())
        .map(sanitize_display_text);
    match record.process_state.as_deref() {
        Some("running") => CommandFacts {
            status: "running".to_string(),
            effective_cwd: lifecycle_cwd,
            exit_code: None,
            termination_reason: None,
            duration_ms: record.duration_ms,
        },
        Some("exited") => CommandFacts {
            status: "exited".to_string(),
            effective_cwd: lifecycle_cwd,
            exit_code: record.process_exit_code,
            termination_reason: record.process_termination_reason.clone(),
            duration_ms: lifecycle_duration(record).or(record.duration_ms),
        },
        Some("incomplete") => CommandFacts {
            status: "incomplete".to_string(),
            effective_cwd: lifecycle_cwd,
            exit_code: None,
            termination_reason: None,
            duration_ms: record.duration_ms,
        },
        _ => {
            if let Some(aggregate) = aggregate
                && aggregate.count > 0
            {
                return CommandFacts {
                    status: aggregate
                        .final_status
                        .clone()
                        .unwrap_or_else(|| status_for(record)),
                    effective_cwd: aggregate
                        .final_cwd
                        .as_deref()
                        .or(record.result_cwd.as_deref())
                        .map(sanitize_display_text),
                    exit_code: aggregate.final_exit_code.or(record.result_exit_code),
                    termination_reason: aggregate
                        .final_termination_reason
                        .clone()
                        .or_else(|| record.result_termination_reason.clone()),
                    duration_ms: aggregate
                        .latest_completed_at_ms
                        .map(|completed| completed.saturating_sub(record.started_at_ms))
                        .or(record.duration_ms),
                };
            }
            let latest = latest_poll.unwrap_or(record);
            CommandFacts {
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
                duration_ms: latest
                    .completed_at_ms
                    .map(|completed| completed.saturating_sub(record.started_at_ms))
                    .or(record.duration_ms),
            }
        }
    }
}

fn lifecycle_duration(record: &PresentationInput) -> Option<i64> {
    Some(
        record
            .process_ended_at_ms?
            .saturating_sub(record.process_started_at_ms?),
    )
}

fn write_stdin_record(record: &PresentationInput) -> PresentationRecord {
    let handle = record
        .target_session_handle
        .as_deref()
        .unwrap_or("<unknown-session>");
    let kind = match continuation_kind(record) {
        Some("kill") => PresentationKind::Kill {
            target_session_handle: sanitize_display_text(handle),
            creator_agent_id: record.target_created_by_agent_id.clone(),
            cross_agent: record.cross_agent == Some(true),
            result_status: Some(status_for(record)),
        },
        Some("stdin") => {
            let Some(chars) = record
                .arguments
                .get("chars")
                .and_then(serde_json::Value::as_str)
            else {
                return generic_record(record);
            };
            let (chars, chars_truncated) = sanitize_preview(chars, MAX_STDIN_PREVIEW_CHARS);
            PresentationKind::Stdin {
                target_session_handle: sanitize_display_text(handle),
                chars,
                chars_truncated,
                creator_agent_id: record.target_created_by_agent_id.clone(),
                cross_agent: record.cross_agent == Some(true),
                result_status: Some(status_for(record)),
            }
        }
        Some("poll") => return generic_record(record),
        _ => return generic_record(record),
    };
    record_with_kind(
        record,
        raw_identity(record.id, 1, iter::once(record.id)),
        record.duration_ms,
        kind,
    )
}

fn orphan_poll_record(
    polls: &[&PresentationInput],
    retained_parent_id: Option<i64>,
) -> PresentationRecord {
    let latest = polls.last().copied().expect("poll group is non-empty");
    let earliest = polls.first().copied().expect("poll group is non-empty");
    let handle = latest
        .target_session_handle
        .as_deref()
        .unwrap_or("<unknown-session>");
    let primary_invocation_id = retained_parent_id.unwrap_or(earliest.id);
    let mut record = record_with_kind(
        latest,
        raw_identity(
            primary_invocation_id,
            polls.len(),
            polls.iter().map(|poll| poll.id),
        ),
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
    record.started_at_ms = earliest.started_at_ms;
    record.evidence = combined_evidence(polls);
    record
}

fn orphan_poll_aggregate_record(
    record: &PresentationInput,
    orphan: &PresentationOrphanPollInput,
) -> PresentationRecord {
    let handle = record
        .target_session_handle
        .as_deref()
        .unwrap_or("<unknown-session>");
    let mut presented = record_with_kind(
        record,
        raw_identity(
            orphan.primary_invocation_id,
            orphan.aggregate.count,
            orphan.aggregate.raw_invocation_ids.iter().copied(),
        ),
        record.duration_ms,
        PresentationKind::PollAggregate {
            target_session_handle: sanitize_display_text(handle),
            count: orphan.aggregate.count,
            final_status: orphan.aggregate.final_status.clone(),
            creator_agent_id: record.target_created_by_agent_id.clone(),
            caller_agent_ids: orphan.aggregate.caller_agent_ids.clone(),
            cross_agent: orphan.aggregate.cross_agent,
        },
    );
    presented.started_at_ms = orphan.started_at_ms;
    presented.evidence = orphan.aggregate.evidence.clone();
    presented
}

fn generic_record(record: &PresentationInput) -> PresentationRecord {
    let summary = record
        .error
        .as_deref()
        .or(record.result_summary.as_deref())
        .map(|summary| sanitize_preview(summary, MAX_GENERIC_SUMMARY_CHARS).0);
    record_with_kind(
        record,
        raw_identity(record.id, 1, iter::once(record.id)),
        record.duration_ms,
        PresentationKind::Generic {
            tool_name: sanitize_display_text(&record.tool_name),
            status: status_for(record),
            summary,
        },
    )
}

#[derive(Debug)]
struct RawIdentity {
    primary_invocation_id: i64,
    raw_evidence_count: usize,
    raw_invocation_ids: Vec<i64>,
    raw_invocation_ids_truncated: bool,
}

fn raw_identity(
    primary_invocation_id: i64,
    raw_evidence_count: usize,
    ids: impl Iterator<Item = i64>,
) -> RawIdentity {
    let raw_invocation_ids = ids
        .take(PRESENTATION_RAW_INVOCATION_ID_SAMPLE_LIMIT)
        .collect::<Vec<_>>();
    RawIdentity {
        primary_invocation_id,
        raw_evidence_count,
        raw_invocation_ids_truncated: raw_evidence_count > raw_invocation_ids.len(),
        raw_invocation_ids,
    }
}

fn record_with_kind(
    record: &PresentationInput,
    identity: RawIdentity,
    duration_ms: Option<i64>,
    kind: PresentationKind,
) -> PresentationRecord {
    PresentationRecord {
        presentation_id: presentation_id_for_root(identity.primary_invocation_id),
        primary_invocation_id: identity.primary_invocation_id,
        raw_evidence_count: identity.raw_evidence_count,
        raw_invocation_ids: identity.raw_invocation_ids,
        raw_invocation_ids_truncated: identity.raw_invocation_ids_truncated,
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

fn evidence_for(record: &PresentationInput) -> PresentationEvidence {
    let lifecycle_incomplete = record.process_state.as_deref() == Some("incomplete");
    let degraded = record.evidence_state == "incomplete"
        || record.capture_state == "incomplete"
        || lifecycle_incomplete;
    let reason = record
        .evidence_reason
        .as_deref()
        .or(record.capture_reason.as_deref())
        .or(record.process_incomplete_reason.as_deref())
        .map(sanitize_display_text);
    PresentationEvidence {
        evidence_state: record.evidence_state.clone(),
        capture_state: record.capture_state.clone(),
        degraded,
        reason,
    }
}

fn combined_evidence(records: &[&PresentationInput]) -> PresentationEvidence {
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

fn combine_evidence_values(
    left: &PresentationEvidence,
    right: &PresentationEvidence,
) -> PresentationEvidence {
    let evidence_state =
        if left.evidence_state == "incomplete" || right.evidence_state == "incomplete" {
            "incomplete"
        } else if left.evidence_state == "pending" || right.evidence_state == "pending" {
            "pending"
        } else {
            "complete"
        };
    let capture_state = if left.capture_state == "incomplete" || right.capture_state == "incomplete"
    {
        "incomplete"
    } else if left.capture_state == "pending" || right.capture_state == "pending" {
        "pending"
    } else if left.capture_state == "complete" || right.capture_state == "complete" {
        "complete"
    } else {
        "not_applicable"
    };
    PresentationEvidence {
        evidence_state: evidence_state.to_string(),
        capture_state: capture_state.to_string(),
        degraded: left.degraded || right.degraded,
        reason: left.reason.clone().or_else(|| right.reason.clone()),
    }
}

fn status_for(record: &PresentationInput) -> String {
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

fn is_poll(record: &PresentationInput) -> bool {
    continuation_kind(record) == Some("poll")
}

fn continuation_kind(record: &PresentationInput) -> Option<&str> {
    if record.tool_name != "write_stdin" {
        return None;
    }
    if let Some(kind) = record.continuation_kind.as_deref() {
        return Some(kind);
    }
    if record
        .arguments
        .get("kill_process")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return Some("kill");
    }
    match record.arguments.get("chars") {
        Some(serde_json::Value::String(chars)) if !chars.is_empty() => Some("stdin"),
        Some(serde_json::Value::String(_) | serde_json::Value::Null) | None => Some("poll"),
        Some(_) => None,
    }
}

fn unique_agent_ids(records: &[&PresentationInput]) -> Vec<String> {
    let mut seen = HashSet::new();
    records
        .iter()
        .filter_map(|record| record.agent_id.clone())
        .filter(|agent_id| seen.insert(agent_id.clone()))
        .collect()
}

fn build_file_changes(record: &PresentationInput) -> Option<Vec<PresentationFileChange>> {
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

    let mut consumed = vec![false; plans.len()];
    let mut changes = Vec::new();
    for index in 0..plans.len() {
        if consumed[index] {
            continue;
        }
        let plan = &plans[index];
        let evidence = &record.file_evidence[index];
        validate_file_change_identity(plan, evidence)?;

        if record.tool_name == "apply_patch" && plan.path_before == plan.path_after {
            let matching = plans
                .iter()
                .enumerate()
                .filter_map(|(candidate_index, candidate)| {
                    (candidate.path_before == plan.path_before
                        && candidate.path_after == plan.path_after)
                        .then_some(candidate_index)
                })
                .collect::<Vec<_>>();
            if matching.len() > 1 {
                let path = &plan.path_before;
                if plans
                    .iter()
                    .enumerate()
                    .any(|(candidate_index, candidate)| {
                        !matching.contains(&candidate_index)
                            && (candidate.path_before == *path || candidate.path_after == *path)
                    })
                {
                    return None;
                }
                let grouped = matching
                    .iter()
                    .map(|candidate_index| {
                        let candidate_plan = &plans[*candidate_index];
                        let candidate_evidence = &record.file_evidence[*candidate_index];
                        validate_file_change_identity(candidate_plan, candidate_evidence)?;
                        Some(candidate_evidence)
                    })
                    .collect::<Option<Vec<_>>>()?;
                changes.push(build_same_path_net_change(path, &grouped)?);
                for candidate_index in matching {
                    consumed[candidate_index] = true;
                }
                continue;
            }
        }

        changes.push(build_file_change(plan, evidence)?);
        consumed[index] = true;
    }
    Some(changes)
}

fn build_file_change(
    plan: &crate::local::file_evidence::FileCapturePlan,
    evidence: &HistoryFileEvidence,
) -> Option<PresentationFileChange> {
    validate_file_change_identity(plan, evidence)?;

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

fn validate_file_change_identity(
    plan: &crate::local::file_evidence::FileCapturePlan,
    evidence: &HistoryFileEvidence,
) -> Option<()> {
    (evidence.source_kind == plan.source.as_str()
        && evidence.operation_hint == plan.operation.as_str()
        && Path::new(&evidence.path_before) == plan.path_before
        && Path::new(&evidence.path_after) == plan.path_after)
        .then_some(())
}

fn build_same_path_net_change(
    path: &Path,
    evidence: &[&HistoryFileEvidence],
) -> Option<PresentationFileChange> {
    let first = *evidence.first()?;
    if evidence.iter().any(|candidate| {
        candidate.path_before != first.path_before
            || candidate.path_after != first.path_after
            || candidate.before_state != first.before_state
            || candidate.before_text != first.before_text
            || candidate.after_state != first.after_state
            || candidate.after_text != first.after_text
            || candidate.destination_before_state.is_some()
            || candidate.destination_before_text.is_some()
            || candidate.destination_before_reason.is_some()
            || candidate.source_after_state.is_some()
            || candidate.source_after_text.is_some()
            || candidate.source_after_reason.is_some()
    }) {
        return None;
    }

    let (operation, before, after) = match (first.before_state.as_str(), first.after_state.as_str())
    {
        ("missing", "text") => (
            PresentationFileOperation::Created,
            "",
            first.after_text.as_deref()?,
        ),
        ("text", "missing") => (
            PresentationFileOperation::Deleted,
            first.before_text.as_deref()?,
            "",
        ),
        ("text", "text") => (
            PresentationFileOperation::Edited,
            first.before_text.as_deref()?,
            first.after_text.as_deref()?,
        ),
        _ => return None,
    };
    let diff = build_text_diff(before, after)?;
    Some(PresentationFileChange {
        operation,
        path: sanitize_display_text(&path.to_string_lossy()),
        old_path: None,
        write_mode: None,
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
