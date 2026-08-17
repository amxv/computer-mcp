use serde_json::json;

use crate::local::{
    HistoryAgentSummary, HistoryAgentWorkdir, HistoryFileEvidence, HistoryFormat,
    HistoryInvocation, LocalHistoryReader,
};

use super::{
    PRESENTATION_SCHEMA_VERSION, PresentationFileOperation, PresentationKind,
    PresentationWriteMode, build_presentation, render_presentation,
};

fn invocation(id: i64, tool: &str, arguments: serde_json::Value) -> HistoryInvocation {
    HistoryInvocation {
        id,
        correlation_id: format!("correlation-{id}"),
        agent_id: Some("k7m2".to_string()),
        provider_kind: Some("openai/session".to_string()),
        provider_session_key: Some("private-provider-key".to_string()),
        tool_name: tool.to_string(),
        arguments,
        declared_workdir_exact: None,
        declared_workdir_normalized: None,
        is_new_workdir: false,
        started_at_ms: id * 100,
        completed_at_ms: Some(id * 100 + 10),
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

fn tool_result(status: &str, output: &str, handle: Option<&str>) -> serde_json::Value {
    json!({
        "summary":"summary",
        "output":output,
        "status":status,
        "cwd":"/tmp/presentation",
        "session_handle":handle,
        "exit_code": if status == "exited" { json!(0) } else { serde_json::Value::Null },
        "termination_reason": if status == "exited" { json!("exit") } else { serde_json::Value::Null }
    })
}

fn evidence(
    ordinal: u32,
    operation: &str,
    paths: (&str, &str),
    before: (&str, Option<&str>),
    after: (&str, Option<&str>),
) -> HistoryFileEvidence {
    let (path_before, path_after) = paths;
    let (before_state, before_text) = before;
    let (after_state, after_text) = after;
    HistoryFileEvidence {
        ordinal,
        source_kind: "apply_patch".to_string(),
        operation_hint: operation.to_string(),
        path_before: path_before.to_string(),
        path_after: path_after.to_string(),
        before_state: before_state.to_string(),
        before_text: before_text.map(str::to_owned),
        before_reason: None,
        destination_before_state: None,
        destination_before_text: None,
        destination_before_reason: None,
        after_state: after_state.to_string(),
        after_text: after_text.map(str::to_owned),
        after_reason: None,
        source_after_state: None,
        source_after_text: None,
        source_after_reason: None,
    }
}

fn shell_evidence(
    operation: &str,
    path: &str,
    before_state: &str,
    before_text: Option<&str>,
    after_text: &str,
) -> HistoryFileEvidence {
    let mut evidence = evidence(
        0,
        operation,
        (path, path),
        (before_state, before_text),
        ("text", Some(after_text)),
    );
    evidence.source_kind = "shell_write".to_string();
    evidence
}

fn agent() -> HistoryAgentSummary {
    HistoryAgentSummary {
        id: "k7m2".to_string(),
        first_seen_at_ms: 100,
        last_seen_at_ms: 900,
        workdirs: vec![
            HistoryAgentWorkdir {
                normalized_workdir: "/repo/one".to_string(),
                ordinal: 1,
                first_seen_at_ms: 100,
                last_seen_at_ms: 500,
                first_invocation_id: 1,
                last_invocation_id: 5,
                retained_invocation_count: 3,
            },
            HistoryAgentWorkdir {
                normalized_workdir: "/repo/two".to_string(),
                ordinal: 2,
                first_seen_at_ms: 600,
                last_seen_at_ms: 900,
                first_invocation_id: 6,
                last_invocation_id: 9,
                retained_invocation_count: 2,
            },
        ],
    }
}

#[test]
fn presentation_is_versioned_agent_aware_and_sanitizes_terminal_controls() {
    let mut record = invocation(
        1,
        "exec_command",
        json!({"cmd":"printf '\u{1b}[31mred\u{1b}[0m'","workdir":"/repo/two"}),
    );
    record.declared_workdir_exact = Some("/repo/two".to_string());
    record.declared_workdir_normalized = Some("/repo/two".to_string());
    record.is_new_workdir = true;
    record.capture_state = "complete".to_string();
    record.result_status = Some("exited".to_string());
    record.result_exit_code = Some(0);
    record.result_cwd = Some("/repo/two".to_string());
    record.result = Some(tool_result(
        "exited",
        "\u{1b}[31mred\u{1b}[0m\u{1b}]8;;https://example.invalid\u{1b}\\link\u{1b}]8;;\u{1b}\\\u{7}\u{202e}spoof\nplain",
        None,
    ));
    let raw_result = record.result.clone();

    let document = build_presentation(&[record.clone()], &[agent()]);
    assert_eq!(document.schema_version, PRESENTATION_SCHEMA_VERSION);
    assert_eq!(
        document.agents[0].workdirs[0].normalized_workdir,
        "/repo/one"
    );
    assert_eq!(
        document.agents[0].workdirs[1].normalized_workdir,
        "/repo/two"
    );
    assert_eq!(
        document.records[0].new_workdir.as_deref(),
        Some("/repo/two")
    );
    let PresentationKind::Command {
        command, output, ..
    } = &document.records[0].kind
    else {
        panic!("expected command presentation");
    };
    assert!(!command.contains('\u{1b}'));
    let output = output.as_deref().unwrap();
    assert!(!output.contains('\u{1b}'));
    assert!(!output.contains('\u{7}'));
    assert!(!output.contains('\u{202e}'));
    assert!(output.contains("\\u{202e}spoof"));
    assert!(output.contains("red"));
    assert!(output.contains("link"));
    assert!(output.contains("plain"));
    assert_eq!(
        record.result, raw_result,
        "presentation must not mutate raw evidence"
    );

    let json = render_presentation(&document, HistoryFormat::Json).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["schema_version"], PRESENTATION_SCHEMA_VERSION);
    assert!(value.get("records").is_some());
    assert!(value.get("provider_session_key").is_none());
    assert!(!json.contains('\u{1b}'));
    assert!(!json.contains('\u{7}'));

    let huge = format!("{}tail", "x".repeat(100_000));
    let (preview, truncated) = super::sanitize::sanitize_preview(&huge, 64);
    assert!(truncated);
    assert_eq!(preview.chars().count(), 65);
}

#[test]
fn apply_patch_file_changes_cover_create_update_delete_move_and_fail_closed() {
    let workdir = "/tmp/presentation";
    let mut create = invocation(
        10,
        "apply_patch",
        json!({
            "workdir":workdir,
            "patch":"*** Begin Patch\n*** Add File: new file.txt\n+one\n+two\n*** End Patch\n"
        }),
    );
    create.file_evidence = vec![evidence(
        0,
        "create",
        (
            "/tmp/presentation/new file.txt",
            "/tmp/presentation/new file.txt",
        ),
        ("missing", None),
        ("text", Some("one\ntwo\n")),
    )];

    let mut update = invocation(
        11,
        "apply_patch",
        json!({
            "workdir":workdir,
            "patch":"*** Begin Patch\n*** Update File: edit.txt\n@@\n-old\n+new\n*** End Patch\n"
        }),
    );
    update.file_evidence = vec![evidence(
        0,
        "update",
        ("/tmp/presentation/edit.txt", "/tmp/presentation/edit.txt"),
        ("text", Some("old\n")),
        ("text", Some("new\n")),
    )];

    let mut delete = invocation(
        12,
        "apply_patch",
        json!({
            "workdir":workdir,
            "patch":"*** Begin Patch\n*** Delete File: gone.txt\n*** End Patch\n"
        }),
    );
    delete.file_evidence = vec![evidence(
        0,
        "delete",
        ("/tmp/presentation/gone.txt", "/tmp/presentation/gone.txt"),
        ("text", Some("gone\n")),
        ("missing", None),
    )];

    let mut moved = invocation(
        13,
        "apply_patch",
        json!({
            "workdir":workdir,
            "patch":"*** Begin Patch\n*** Update File: old name.txt\n*** Move to: new name.txt\n@@\n-old\n+new\n*** End Patch\n"
        }),
    );
    let mut move_evidence = evidence(
        0,
        "move",
        (
            "/tmp/presentation/old name.txt",
            "/tmp/presentation/new name.txt",
        ),
        ("text", Some("old\n")),
        ("text", Some("new\n")),
    );
    move_evidence.destination_before_state = Some("missing".to_string());
    move_evidence.source_after_state = Some("missing".to_string());
    moved.file_evidence = vec![move_evidence];

    let document = build_presentation(&[create.clone(), update, delete, moved.clone()], &[]);
    let operations = document
        .records
        .iter()
        .map(|record| match &record.kind {
            PresentationKind::FileChanges { changes, .. } => changes[0].operation,
            _ => panic!("expected file change"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        operations,
        vec![
            PresentationFileOperation::Created,
            PresentationFileOperation::Edited,
            PresentationFileOperation::Deleted,
            PresentationFileOperation::Renamed,
        ]
    );
    let PresentationKind::FileChanges { changes, .. } = &document.records[1].kind else {
        unreachable!()
    };
    assert_eq!(changes[0].added, 1);
    assert_eq!(changes[0].removed, 1);
    assert!(changes[0].lines.iter().any(|line| line.kind == "add"));
    assert!(changes[0].lines.iter().any(|line| line.kind == "remove"));

    let mut overwrite_destination = moved.clone();
    overwrite_destination.file_evidence[0].destination_before_state = Some("text".to_string());
    overwrite_destination.file_evidence[0].destination_before_text =
        Some("old destination\n".to_string());
    let document = build_presentation(&[overwrite_destination], &[]);
    assert!(matches!(
        document.records[0].kind,
        PresentationKind::Generic { .. }
    ));

    let mut contradictory = create;
    contradictory.file_evidence[0].after_state = "unavailable".to_string();
    contradictory.file_evidence[0].after_text = None;
    let document = build_presentation(&[contradictory], &[]);
    assert!(matches!(
        document.records[0].kind,
        PresentationKind::Generic { .. }
    ));

    let mut add_overwrite = invocation(
        15,
        "apply_patch",
        json!({
            "workdir":workdir,
            "patch":"*** Begin Patch\n*** Add File: existing.txt\n+new\n*** End Patch\n"
        }),
    );
    add_overwrite.file_evidence = vec![evidence(
        0,
        "create",
        (
            "/tmp/presentation/existing.txt",
            "/tmp/presentation/existing.txt",
        ),
        ("text", Some("old\n")),
        ("text", Some("new\n")),
    )];
    let document = build_presentation(&[add_overwrite], &[]);
    assert!(matches!(
        document.records[0].kind,
        PresentationKind::Generic { .. }
    ));

    let mut multi = invocation(
        14,
        "apply_patch",
        json!({
            "workdir":workdir,
            "patch":"*** Begin Patch\n*** Add File: first.txt\n+first\n*** Add File: second file.txt\n+second\n*** End Patch\n"
        }),
    );
    multi.file_evidence = vec![
        evidence(
            0,
            "create",
            ("/tmp/presentation/first.txt", "/tmp/presentation/first.txt"),
            ("missing", None),
            ("text", Some("first\n")),
        ),
        evidence(
            1,
            "create",
            (
                "/tmp/presentation/second file.txt",
                "/tmp/presentation/second file.txt",
            ),
            ("missing", None),
            ("text", Some("second\n")),
        ),
    ];
    let document = build_presentation(&[multi], &[]);
    let PresentationKind::FileChanges { changes, .. } = &document.records[0].kind else {
        panic!("multi-file patch should remain one invocation with multiple file changes");
    };
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[1].path, "/tmp/presentation/second file.txt");

    let mut recreate = invocation(
        16,
        "apply_patch",
        json!({
            "workdir":workdir,
            "patch":"*** Begin Patch\n*** Delete File: recreated.txt\n*** Add File: recreated.txt\n+new\n*** Update File: other.txt\n@@\n-old\n+new other\n*** End Patch\n"
        }),
    );
    recreate.file_evidence = vec![
        evidence(
            0,
            "delete",
            (
                "/tmp/presentation/recreated.txt",
                "/tmp/presentation/recreated.txt",
            ),
            ("text", Some("old\n")),
            ("text", Some("new\n")),
        ),
        evidence(
            1,
            "create",
            (
                "/tmp/presentation/recreated.txt",
                "/tmp/presentation/recreated.txt",
            ),
            ("text", Some("old\n")),
            ("text", Some("new\n")),
        ),
        evidence(
            2,
            "update",
            ("/tmp/presentation/other.txt", "/tmp/presentation/other.txt"),
            ("text", Some("old\n")),
            ("text", Some("new other\n")),
        ),
    ];
    let document = build_presentation(&[recreate.clone()], &[]);
    let PresentationKind::FileChanges { changes, .. } = &document.records[0].kind else {
        panic!("same-path delete+recreate should render its net file changes");
    };
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].operation, PresentationFileOperation::Edited);
    assert_eq!(changes[0].path, "/tmp/presentation/recreated.txt");
    assert_eq!(changes[0].added, 1);
    assert_eq!(changes[0].removed, 1);
    assert_eq!(changes[1].path, "/tmp/presentation/other.txt");

    recreate.file_evidence[1].after_text = Some("contradictory\n".to_string());
    let document = build_presentation(&[recreate], &[]);
    assert!(matches!(
        document.records[0].kind,
        PresentationKind::Generic { .. }
    ));
}

#[test]
fn shell_write_requires_completed_actual_evidence_and_preserves_append_vs_overwrite() {
    let workdir = "/tmp/presentation";
    let path = "/tmp/presentation/file.txt";
    let mut append = invocation(
        20,
        "exec_command",
        json!({
            "cmd":"cat >> file.txt <<'EOF'\nb\nEOF\n",
            "workdir":workdir
        }),
    );
    append.capture_state = "complete".to_string();
    append.result_status = Some("exited".to_string());
    append.result = Some(tool_result("exited", "", None));
    append.file_evidence = vec![shell_evidence(
        "append",
        path,
        "text",
        Some("a\n"),
        "a\nb\n",
    )];

    let mut overwrite = invocation(
        21,
        "exec_command",
        json!({"cmd":"printf 'x\\n' > file.txt","workdir":workdir}),
    );
    overwrite.capture_state = "complete".to_string();
    overwrite.result_status = Some("exited".to_string());
    overwrite.result = Some(tool_result("exited", "", None));
    overwrite.file_evidence = vec![shell_evidence("overwrite", path, "missing", None, "x\n")];

    let document = build_presentation(&[append.clone(), overwrite], &[]);
    let PresentationKind::FileChanges { changes, .. } = &document.records[0].kind else {
        panic!("append should be a file edit");
    };
    assert_eq!(changes[0].write_mode, Some(PresentationWriteMode::Append));
    assert_eq!(changes[0].operation, PresentationFileOperation::Edited);
    let PresentationKind::FileChanges { changes, .. } = &document.records[1].kind else {
        panic!("overwrite should be a file edit");
    };
    assert_eq!(
        changes[0].write_mode,
        Some(PresentationWriteMode::Overwrite)
    );
    assert_eq!(changes[0].operation, PresentationFileOperation::Created);

    let mut mismatch = append.clone();
    mismatch.file_evidence[0].after_text = Some("not-an-append\n".to_string());
    let document = build_presentation(&[mismatch], &[]);
    assert!(matches!(
        document.records[0].kind,
        PresentationKind::Command { .. }
    ));

    let mut running = append;
    running.result_status = Some("running".to_string());
    let document = build_presentation(&[running], &[]);
    assert!(matches!(
        document.records[0].kind,
        PresentationKind::Command { .. }
    ));
}

#[test]
fn fifty_cumulative_null_and_empty_string_polls_collapse_into_one_parent_command() {
    let handle = "poll-session-handle";
    let mut parent = invocation(
        30,
        "exec_command",
        json!({"cmd":"long-task","workdir":"/tmp/presentation"}),
    );
    parent.capture_state = "complete".to_string();
    parent.result = Some(tool_result("running", "start", Some(handle)));
    parent.result_status = Some("running".to_string());
    parent.result_session_handle = Some(handle.to_string());
    parent.output_preview = Some("full-start\nfull-middle\nfull-end\n".to_string());

    let mut records = Vec::new();
    for index in 1..=50 {
        let mut poll = invocation(
            30 + index,
            "write_stdin",
            json!({
                "session_handle": handle,
                "chars": if index % 2 == 0 { json!("") } else { serde_json::Value::Null },
                "kill_process": false
            }),
        );
        poll.target_session_handle = Some(handle.to_string());
        poll.result_status = Some(if index == 50 { "exited" } else { "running" }.to_string());
        poll.result = Some(tool_result(
            if index == 50 { "exited" } else { "running" },
            &format!("cumulative-{index}"),
            if index == 50 { None } else { Some(handle) },
        ));
        poll.started_at_ms = 3_000 + index;
        records.push(poll);
    }
    records.reverse();
    records.push(parent);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.tool_name == "write_stdin")
            .count(),
        50
    );

    let document = build_presentation(&records, &[]);
    assert_eq!(document.records.len(), 1);
    let PresentationKind::Command {
        polls,
        status,
        output,
        ..
    } = &document.records[0].kind
    else {
        panic!("polls should fold into parent command");
    };
    let polls = polls.as_ref().unwrap();
    assert_eq!(polls.count, 50);
    assert_eq!(polls.final_status.as_deref(), Some("exited"));
    assert_eq!(status, "exited");
    assert_eq!(
        output.as_deref(),
        Some("full-start\nfull-middle\nfull-end\n")
    );
    assert_eq!(document.records[0].raw_evidence_count, 51);
    assert_eq!(document.records[0].raw_invocation_ids.len(), 32);
    assert!(document.records[0].raw_invocation_ids_truncated);
    let markdown = render_presentation(&document, HistoryFormat::Markdown).unwrap();
    assert_eq!(markdown.matches("polled 50x").count(), 1);
    assert!(markdown.contains("full-middle"));
    assert!(!markdown.contains("cumulative-50"));
    assert!(!markdown.contains("cumulative-49"));
    let raw = LocalHistoryReader::render(&records, HistoryFormat::Json, true).unwrap();
    let raw_value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        raw_value.as_array().unwrap().len(),
        51,
        "presentation folding must never delete the 50 durable raw poll invocations"
    );
}

#[test]
fn orphan_poll_page_still_collapses_without_inventing_a_parent() {
    let handle = "orphan-poll-handle";
    let mut polls = Vec::new();
    for index in 0..3 {
        let mut poll = invocation(
            200 + index,
            "write_stdin",
            json!({"session_handle":handle,"chars":null,"kill_process":false}),
        );
        poll.target_session_handle = Some(handle.to_string());
        poll.result_status = Some("running".to_string());
        polls.push(poll);
    }
    polls.reverse();
    let document = build_presentation(&polls, &[]);
    assert_eq!(document.records.len(), 1);
    assert!(matches!(
        &document.records[0].kind,
        PresentationKind::PollAggregate {
            count: 3,
            target_session_handle,
            ..
        } if target_session_handle == handle
    ));
}

#[test]
fn file_diff_display_is_bounded_without_losing_full_stats() {
    let workdir = "/tmp/presentation";
    let before = (0..900)
        .map(|index| format!("old-{index}\n"))
        .collect::<String>();
    let after = (0..900)
        .map(|index| format!("new-{index}\n"))
        .collect::<String>();
    let mut record = invocation(
        250,
        "apply_patch",
        json!({
            "workdir":workdir,
            "patch":"*** Begin Patch\n*** Update File: huge.txt\n@@\n-old\n+new\n*** End Patch\n"
        }),
    );
    record.file_evidence = vec![evidence(
        0,
        "update",
        ("/tmp/presentation/huge.txt", "/tmp/presentation/huge.txt"),
        ("text", Some(&before)),
        ("text", Some(&after)),
    )];
    let document = build_presentation(&[record], &[]);
    let PresentationKind::FileChanges { changes, .. } = &document.records[0].kind else {
        panic!("expected file changes");
    };
    assert_eq!(changes[0].added, 900);
    assert_eq!(changes[0].removed, 900);
    assert!(changes[0].diff_truncated);
    assert_eq!(changes[0].lines.len(), 500);

    let long_before = "a".repeat(10_000);
    let long_after = "b".repeat(10_000);
    let diff = super::diff::build_text_diff(&long_before, &long_after).unwrap();
    assert_eq!(diff.added, 1);
    assert_eq!(diff.removed, 1);
    assert!(diff.truncated);
    assert!(
        diff.lines
            .iter()
            .all(|line| line.text.chars().count() <= 2_049)
    );
}

#[test]
fn pathological_many_line_diff_falls_back_before_expensive_diffing() {
    let workdir = "/tmp/presentation";
    let before = (0..2_100).map(|_| "old\n").collect::<String>();
    let after = (0..2_100).map(|_| "new\n").collect::<String>();
    let mut record = invocation(
        251,
        "apply_patch",
        json!({
            "workdir":workdir,
            "patch":"*** Begin Patch\n*** Update File: pathological.txt\n@@\n-old\n+new\n*** End Patch\n"
        }),
    );
    record.file_evidence = vec![evidence(
        0,
        "update",
        (
            "/tmp/presentation/pathological.txt",
            "/tmp/presentation/pathological.txt",
        ),
        ("text", Some(&before)),
        ("text", Some(&after)),
    )];

    let document = build_presentation(&[record], &[]);
    assert!(matches!(
        document.records[0].kind,
        PresentationKind::Generic { .. }
    ));
}

#[test]
fn stdin_and_kill_remain_visible_with_cross_agent_attribution() {
    let handle = "cross-agent-handle";
    let mut stdin = invocation(
        90,
        "write_stdin",
        json!({"session_handle":handle,"chars":"yes\n","kill_process":false}),
    );
    stdin.agent_id = Some("b222".to_string());
    stdin.target_session_handle = Some(handle.to_string());
    stdin.target_created_by_agent_id = Some("a111".to_string());
    stdin.cross_agent = Some(true);
    stdin.result_status = Some("running".to_string());

    let mut kill = invocation(
        91,
        "write_stdin",
        json!({"session_handle":handle,"kill_process":true}),
    );
    kill.agent_id = Some("b222".to_string());
    kill.target_session_handle = Some(handle.to_string());
    kill.target_created_by_agent_id = Some("a111".to_string());
    kill.cross_agent = Some(true);
    kill.result_status = Some("exited".to_string());

    let document = build_presentation(&[kill, stdin], &[]);
    assert_eq!(document.records.len(), 2);
    assert!(matches!(
        &document.records[0].kind,
        PresentationKind::Kill {
            creator_agent_id: Some(id),
            cross_agent: true,
            ..
        } if id == "a111"
    ));
    assert!(matches!(
        &document.records[1].kind,
        PresentationKind::Stdin {
            chars,
            creator_agent_id: Some(id),
            cross_agent: true,
            ..
        } if chars == "yes\n" && id == "a111"
    ));
}

#[test]
fn command_state_inherits_degraded_evidence_from_a_folded_poll() {
    let handle = "continuation-evidence-handle";
    let mut parent = invocation(
        95,
        "exec_command",
        json!({"cmd":"long-task","workdir":"/tmp/presentation"}),
    );
    parent.result_status = Some("running".to_string());
    parent.result_session_handle = Some(handle.to_string());
    parent.result = Some(tool_result("running", "started", Some(handle)));

    let mut poll = invocation(
        96,
        "write_stdin",
        json!({"session_handle":handle,"chars":null,"kill_process":false}),
    );
    poll.target_session_handle = Some(handle.to_string());
    poll.target_created_by_invocation_id = Some(parent.id);
    poll.continuation_kind = Some("poll".to_string());
    poll.result_status = Some("exited".to_string());
    poll.capture_state = "incomplete".to_string();
    poll.capture_reason = Some("injected continuation capture loss".to_string());

    let document = build_presentation(&[parent, poll], &[]);
    let command = document
        .records
        .iter()
        .find(|record| matches!(record.kind, PresentationKind::Command { .. }))
        .unwrap();
    assert!(command.evidence.degraded);
    assert_eq!(
        command.evidence.reason.as_deref(),
        Some("injected continuation capture loss")
    );
    assert_eq!(document.records.len(), 1, "poll must stay folded");
}

#[test]
fn failed_action_does_not_misstate_parent_process_status() {
    let handle = "failed-action-handle";
    let mut parent = invocation(
        97,
        "exec_command",
        json!({"cmd":"long-task","workdir":"/tmp/presentation"}),
    );
    parent.result_status = Some("running".to_string());
    parent.result_session_handle = Some(handle.to_string());
    parent.result = Some(tool_result("running", "started", Some(handle)));

    let mut stdin = invocation(
        98,
        "write_stdin",
        json!({"session_handle":handle,"chars":"y\n","kill_process":false}),
    );
    stdin.target_session_handle = Some(handle.to_string());
    stdin.outcome_kind = Some("error".to_string());
    stdin.error = Some("write failed".to_string());

    let document = build_presentation(&[parent, stdin], &[]);
    let command = document
        .records
        .iter()
        .find_map(|record| match &record.kind {
            PresentationKind::Command { status, .. } => Some(status.as_str()),
            _ => None,
        })
        .unwrap();
    assert_eq!(command, "running");
    let action_status = document
        .records
        .iter()
        .find_map(|record| match &record.kind {
            PresentationKind::Stdin { result_status, .. } => result_status.as_deref(),
            _ => None,
        });
    assert_eq!(action_status, Some("failed"));
}

#[test]
fn degraded_capture_is_explicit_and_raw_json_remains_raw() {
    let mut record = invocation(
        100,
        "exec_command",
        json!({"cmd":"echo hi","workdir":"/tmp/presentation"}),
    );
    record.capture_state = "incomplete".to_string();
    record.capture_reason = Some("injected capture loss".to_string());
    record.declared_workdir_normalized = Some("/tmp/\u{1b}[31mraw\u{202e}path".to_string());
    record.arguments["note"] = json!("raw\u{202e}argument");
    record.result_status = Some("exited".to_string());
    record.result = Some(tool_result("exited", "hi\n", None));

    let document = build_presentation(&[record.clone()], &[]);
    assert!(document.records[0].evidence.degraded);
    let markdown = render_presentation(&document, HistoryFormat::Markdown).unwrap();
    assert!(markdown.contains("incomplete evidence"));
    assert!(markdown.contains("injected capture loss"));

    let raw = LocalHistoryReader::render(&[record.clone()], HistoryFormat::Json, true).unwrap();
    let raw_value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(
        raw_value.is_array(),
        "--raw must retain the Phase-5 raw array contract"
    );
    assert!(raw_value[0].get("provider_session_key").is_some());
    assert!(raw_value[0].get("schema_version").is_none());

    let raw_markdown =
        LocalHistoryReader::render(&[record], HistoryFormat::Markdown, true).unwrap();
    assert!(!raw_markdown.contains('\u{1b}'));
    assert!(!raw_markdown.contains('\u{202e}'));
    assert!(raw_markdown.contains("\\u{202e}"));
}

#[test]
fn markdown_uses_safe_code_spans_for_backticks_in_commands_and_paths() {
    let mut record = invocation(
        300,
        "exec_command",
        json!({"cmd":"printf '`tick`'","workdir":"/tmp/back`tick"}),
    );
    record.declared_workdir_exact = Some("/tmp/back`tick".to_string());
    record.declared_workdir_normalized = Some("/tmp/back`tick".to_string());
    record.result_status = Some("exited".to_string());
    record.result = Some(tool_result("exited", "`output`\n", None));

    let document = build_presentation(&[record], &[]);
    let markdown = render_presentation(&document, HistoryFormat::Markdown).unwrap();
    assert!(markdown.contains("``$ printf '`tick`'``"), "{markdown}");
    assert!(markdown.contains("``/tmp/back`tick``"), "{markdown}");
}
