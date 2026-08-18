use serde_json::json;

use crate::local::{HistoryFileEvidence, HistoryInvocation};

use super::{PresentationKind, build_presentation};

fn invocation(id: i64, tool_name: &str, arguments: serde_json::Value) -> HistoryInvocation {
    HistoryInvocation {
        id,
        correlation_id: format!("corr-{id}"),
        agent_id: Some("k7m2".to_string()),
        provider_kind: Some("openai/session".to_string()),
        provider_session_key: Some("provider-secret-must-not-leak".to_string()),
        tool_name: tool_name.to_string(),
        arguments,
        declared_workdir_exact: Some("/tmp/presentation".to_string()),
        declared_workdir_normalized: Some("/tmp/presentation".to_string()),
        is_new_workdir: false,
        started_at_ms: id.saturating_mul(100),
        completed_at_ms: Some(id.saturating_mul(100).saturating_add(10)),
        duration_ms: Some(10),
        outcome_kind: Some("success".to_string()),
        result: None,
        error: None,
        evidence_state: "complete".to_string(),
        evidence_reason: None,
        capture_state: "not_applicable".to_string(),
        capture_reason: None,
        target_session_handle: None,
        target_created_by_agent_id: None,
        target_created_by_invocation_id: None,
        continuation_kind: None,
        cross_agent: None,
        result_status: None,
        result_cwd: None,
        result_session_handle: None,
        result_exit_code: None,
        result_termination_reason: None,
        process_state: None,
        process_started_at_ms: None,
        process_ended_at_ms: None,
        process_updated_at_ms: None,
        process_exit_code: None,
        process_termination_reason: None,
        process_cwd: None,
        process_incomplete_reason: None,
        output_preview: None,
        output_preview_truncated: false,
        full_output: None,
        file_evidence: Vec::new(),
    }
}

fn running_command(id: i64, handle: &str) -> HistoryInvocation {
    let mut record = invocation(
        id,
        "exec_command",
        json!({"cmd":"long-task","workdir":"/tmp/presentation"}),
    );
    record.capture_state = "complete".to_string();
    record.result_status = Some("running".to_string());
    record.result_cwd = Some("/tmp/presentation".to_string());
    record.result_session_handle = Some(handle.to_string());
    record.result = Some(json!({
        "summary":"still running",
        "output":"logical-running-output",
        "status":"running",
        "cwd":"/tmp/presentation",
        "session_handle":handle,
        "exit_code":null,
        "termination_reason":null
    }));
    record
}

fn continuation(
    id: i64,
    parent_id: Option<i64>,
    handle: &str,
    kind: &str,
    chars: Option<&str>,
    status: &str,
) -> HistoryInvocation {
    let mut record = invocation(
        id,
        "write_stdin",
        json!({
            "session_handle": handle,
            "chars": chars,
            "kill_process": kind == "kill"
        }),
    );
    record.target_session_handle = Some(handle.to_string());
    record.target_created_by_agent_id = Some("k7m2".to_string());
    record.target_created_by_invocation_id = parent_id;
    record.continuation_kind = Some(kind.to_string());
    record.result_status = Some(status.to_string());
    record.result_cwd = Some("/tmp/presentation".to_string());
    record.result_exit_code = (status == "exited").then_some(0);
    record.result_termination_reason = (status == "exited").then(|| "exit".to_string());
    record.result = Some(json!({
        "summary":status,
        "output":format!("cumulative-{id}"),
        "status":status,
        "cwd":"/tmp/presentation",
        "session_handle": if status == "running" { Some(handle) } else { None },
        "exit_code": if status == "exited" { Some(0) } else { None },
        "termination_reason": if status == "exited" { Some("exit") } else { None }
    }));
    record
}

#[test]
fn durable_lifecycle_upgrades_running_logical_evidence_without_mutating_it() {
    let handle = "lifecycle-handle";
    let mut parent = running_command(10, handle);
    let raw_result = parent.result.clone();
    parent.process_state = Some("exited".to_string());
    parent.process_started_at_ms = Some(1_000);
    parent.process_ended_at_ms = Some(5_250);
    parent.process_updated_at_ms = Some(5_250);
    parent.process_exit_code = Some(7);
    parent.process_termination_reason = Some("exit".to_string());
    parent.process_cwd = Some("/tmp/presentation/final".to_string());

    let document = build_presentation(&[parent.clone()], &[]);
    let record = &document.records[0];
    assert_eq!(record.presentation_id, "inv-10");
    assert_eq!(record.primary_invocation_id, 10);
    assert_eq!(record.raw_evidence_count, 1);
    let PresentationKind::Command {
        status,
        effective_cwd,
        exit_code,
        termination_reason,
        ..
    } = &record.kind
    else {
        panic!("expected command");
    };
    assert_eq!(status, "exited");
    assert_eq!(effective_cwd.as_deref(), Some("/tmp/presentation/final"));
    assert_eq!(*exit_code, Some(7));
    assert_eq!(termination_reason.as_deref(), Some("exit"));
    assert_eq!(record.duration_ms, Some(4_250));
    assert_eq!(parent.result_status.as_deref(), Some("running"));
    assert_eq!(parent.result, raw_result);
}

#[test]
fn stdin_and_kill_remain_independent_and_never_substitute_parent_status() {
    let handle = "interaction-handle";
    let parent = running_command(20, handle);
    let stdin = continuation(21, Some(20), handle, "stdin", Some("y\n"), "exited");
    let kill = continuation(22, Some(20), handle, "kill", None, "exited");

    let document = build_presentation(&[parent, stdin, kill], &[]);
    assert_eq!(document.records.len(), 3);
    let command = document
        .records
        .iter()
        .find(|record| record.primary_invocation_id == 20)
        .unwrap();
    let PresentationKind::Command { status, polls, .. } = &command.kind else {
        panic!("expected command");
    };
    assert_eq!(status, "running");
    assert!(polls.is_none());
    assert_eq!(command.raw_evidence_count, 1);
    assert!(document.records.iter().any(|record| {
        record.primary_invocation_id == 21 && matches!(record.kind, PresentationKind::Stdin { .. })
    }));
    assert!(document.records.iter().any(|record| {
        record.primary_invocation_id == 22 && matches!(record.kind, PresentationKind::Kill { .. })
    }));
}

#[test]
fn incomplete_lifecycle_is_explicit_degraded_truth_not_forever_running() {
    let mut parent = running_command(30, "incomplete-handle");
    parent.process_state = Some("incomplete".to_string());
    parent.process_incomplete_reason = Some(
        "previous Local runtime ended before final process lifecycle was observed".to_string(),
    );

    let document = build_presentation(&[parent], &[]);
    let record = &document.records[0];
    let PresentationKind::Command {
        status, exit_code, ..
    } = &record.kind
    else {
        panic!("expected command");
    };
    assert_eq!(status, "incomplete");
    assert!(exit_code.is_none());
    assert!(record.evidence.degraded);
    assert!(
        record
            .evidence
            .reason
            .as_deref()
            .unwrap()
            .contains("previous Local runtime ended")
    );
}

#[test]
fn stable_ids_survive_appended_polls_and_orphan_aggregates_use_provable_roots() {
    let handle = "stable-handle";
    let parent = running_command(40, handle);
    let poll_one = continuation(41, Some(40), handle, "poll", None, "running");
    let poll_two = continuation(42, Some(40), handle, "poll", None, "running");

    let first = build_presentation(&[parent.clone(), poll_one.clone()], &[]);
    let second = build_presentation(&[parent, poll_one.clone(), poll_two.clone()], &[]);
    assert_eq!(first.records[0].presentation_id, "inv-40");
    assert_eq!(second.records[0].presentation_id, "inv-40");
    assert_eq!(first.records[0].primary_invocation_id, 40);
    assert_eq!(second.records[0].primary_invocation_id, 40);

    let orphan_one = build_presentation(std::slice::from_ref(&poll_one), &[]);
    let orphan_two = build_presentation(&[poll_one, poll_two], &[]);
    assert_eq!(orphan_one.records[0].presentation_id, "inv-40");
    assert_eq!(orphan_two.records[0].presentation_id, "inv-40");
    assert_eq!(orphan_two.records[0].primary_invocation_id, 40);

    let legacy_one = continuation(50, None, "legacy-handle", "poll", None, "running");
    let legacy_two = continuation(51, None, "legacy-handle", "poll", None, "running");
    let legacy_first = build_presentation(std::slice::from_ref(&legacy_one), &[]);
    let legacy_appended = build_presentation(&[legacy_one, legacy_two], &[]);
    assert_eq!(legacy_first.records[0].presentation_id, "inv-50");
    assert_eq!(legacy_appended.records[0].presentation_id, "inv-50");
}

#[test]
fn pathological_poll_count_stays_one_card_with_exact_count_and_bounded_id_sample() {
    let handle = "many-polls";
    let parent = running_command(100, handle);
    let mut records = Vec::with_capacity(1_001);
    records.push(parent);
    for offset in 1..=1_000 {
        records.push(continuation(
            100 + offset,
            Some(100),
            handle,
            "poll",
            None,
            if offset == 1_000 { "exited" } else { "running" },
        ));
    }

    let document = build_presentation(&records, &[]);
    assert_eq!(document.records.len(), 1);
    let record = &document.records[0];
    let PresentationKind::Command { polls, .. } = &record.kind else {
        panic!("expected command");
    };
    assert_eq!(polls.as_ref().unwrap().count, 1_000);
    assert_eq!(record.raw_evidence_count, 1_001);
    assert_eq!(record.raw_invocation_ids.len(), 32);
    assert!(record.raw_invocation_ids_truncated);
    assert_eq!(record.raw_invocation_ids[0], 100);
    let encoded = serde_json::to_string(&document).unwrap();
    assert!(
        encoded.len() < 16_000,
        "presentation JSON grew with poll count"
    );
    assert!(!encoded.contains("provider-secret-must-not-leak"));
    assert!(!encoded.contains("many-polls\""));
}

#[test]
fn later_process_exit_does_not_retroactively_validate_running_shell_write_snapshot() {
    let path = "/tmp/presentation/file.txt";
    let mut parent = running_command(70, "shell-write-handle");
    parent.arguments = json!({
        "cmd":"printf 'new\\n' > file.txt",
        "workdir":"/tmp/presentation"
    });
    parent.process_state = Some("exited".to_string());
    parent.process_started_at_ms = Some(7_000);
    parent.process_ended_at_ms = Some(7_500);
    parent.process_exit_code = Some(0);
    parent.process_termination_reason = Some("exit".to_string());
    parent.file_evidence = vec![HistoryFileEvidence {
        ordinal: 0,
        source_kind: "shell_write".to_string(),
        operation_hint: "overwrite".to_string(),
        path_before: path.to_string(),
        path_after: path.to_string(),
        before_state: "text".to_string(),
        before_text: Some("old\n".to_string()),
        before_reason: None,
        destination_before_state: None,
        destination_before_text: None,
        destination_before_reason: None,
        after_state: "text".to_string(),
        after_text: Some("new\n".to_string()),
        after_reason: None,
        source_after_state: None,
        source_after_text: None,
        source_after_reason: None,
    }];

    let document = build_presentation(&[parent], &[]);
    assert!(matches!(
        document.records[0].kind,
        PresentationKind::Command { .. }
    ));
    let PresentationKind::Command { status, .. } = &document.records[0].kind else {
        unreachable!()
    };
    assert_eq!(
        status, "exited",
        "lifecycle status may advance independently"
    );
}

#[test]
fn presentation_ids_are_bounded_url_path_safe_and_secret_free() {
    let record = running_command(i64::MAX, "public-handle");
    let document = build_presentation(&[record], &[]);
    let id = &document.records[0].presentation_id;
    assert!(id.len() <= 32);
    assert!(
        id.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    );
    let encoded = serde_json::to_string(&document).unwrap();
    assert!(!encoded.contains("provider-secret-must-not-leak"));
    assert!(!encoded.contains("openai/session"));
}

#[test]
fn representative_presentation_records_serialize_without_provider_secrets() {
    let handle = "representative-handle";
    let command = running_command(200, handle);
    let stdin = continuation(201, Some(200), handle, "stdin", Some("y\n"), "running");
    let kill = continuation(202, Some(200), handle, "kill", None, "exited");
    let orphan = continuation(203, Some(999), "orphan-handle", "poll", None, "running");
    let mut file = invocation(
        204,
        "apply_patch",
        json!({
            "workdir":"/tmp/presentation",
            "patch":"*** Begin Patch\n*** Add File: representative.txt\n+hello\n*** End Patch\n"
        }),
    );
    file.file_evidence = vec![HistoryFileEvidence {
        ordinal: 0,
        source_kind: "apply_patch".to_string(),
        operation_hint: "create".to_string(),
        path_before: "/tmp/presentation/representative.txt".to_string(),
        path_after: "/tmp/presentation/representative.txt".to_string(),
        before_state: "missing".to_string(),
        before_text: None,
        before_reason: None,
        destination_before_state: None,
        destination_before_text: None,
        destination_before_reason: None,
        after_state: "text".to_string(),
        after_text: Some("hello\n".to_string()),
        after_reason: None,
        source_after_state: None,
        source_after_text: None,
        source_after_reason: None,
    }];

    let document = build_presentation(&[command, stdin, kill, orphan, file], &[]);
    assert!(
        document
            .records
            .iter()
            .any(|record| { matches!(record.kind, PresentationKind::Command { .. }) })
    );
    assert!(
        document
            .records
            .iter()
            .any(|record| { matches!(record.kind, PresentationKind::Stdin { .. }) })
    );
    assert!(
        document
            .records
            .iter()
            .any(|record| { matches!(record.kind, PresentationKind::Kill { .. }) })
    );
    assert!(
        document
            .records
            .iter()
            .any(|record| { matches!(record.kind, PresentationKind::PollAggregate { .. }) })
    );
    assert!(
        document
            .records
            .iter()
            .any(|record| { matches!(record.kind, PresentationKind::FileChanges { .. }) })
    );
    let encoded = serde_json::to_string(&document).unwrap();
    assert!(!encoded.contains("provider-secret-must-not-leak"));
    assert!(!encoded.contains("openai/session"));
    for record in &document.records {
        assert!(record.presentation_id.len() <= 32);
        assert!(
            record
                .presentation_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        );
    }
}
