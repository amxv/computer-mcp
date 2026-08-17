use serde_json::json;
use std::time::{Duration, Instant};

use super::super::history::{HISTORY_LIVE_EVENT_SCHEMA_VERSION, HistoryLiveEvent};
use super::super::{
    PRESENTATION_SCHEMA_VERSION, PresentationDocument, PresentationKind, PresentationPollSummary,
};
use super::input::WatchInput;
use super::model::{ConnectionState, WatchApp, WatchEffect, WatchOptions, WatchScope};
use super::test_support::{RUNTIME_ID, agent, bootstrap, command_detail, poll_detail};

fn automatic() -> WatchOptions {
    WatchOptions::automatic()
}

#[test]
fn initial_agent_modes_are_waiting_direct_or_picker() {
    let waiting = WatchApp::new(&bootstrap(Vec::new()), automatic());
    assert_eq!(waiting.scope, WatchScope::Waiting);
    assert_eq!(waiting.stream_filter(), None);

    let direct = WatchApp::new(&bootstrap(vec![agent("k7m2", &["/one"])]), automatic());
    assert_eq!(direct.scope, WatchScope::Agent("k7m2".to_owned()));
    assert_eq!(direct.stream_filter().as_deref(), Some("k7m2"));

    let picker = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/one"]), agent("m4n8", &["/two"])]),
        automatic(),
    );
    assert_eq!(picker.scope, WatchScope::Picker);
    assert_eq!(picker.stream_filter(), None);
    assert_eq!(picker.picker_index, 1, "first Agent is the picker default");
}

#[test]
fn explicit_agent_and_all_modes_do_not_auto_switch() {
    let mut dedicated = WatchApp::new(
        &bootstrap(Vec::new()),
        WatchOptions {
            agent: Some("k7m2".to_owned()),
            all: false,
        },
    );
    assert_eq!(dedicated.scope, WatchScope::Agent("k7m2".to_owned()));
    assert!(
        dedicated
            .set_agents(vec![agent("m4n8", &["/two"])])
            .is_empty()
    );
    assert_eq!(dedicated.scope, WatchScope::Agent("k7m2".to_owned()));

    let all = WatchApp::new(
        &bootstrap(Vec::new()),
        WatchOptions {
            agent: None,
            all: true,
        },
    );
    assert_eq!(all.scope, WatchScope::All);
}

#[test]
fn agent_refresh_does_not_replace_runtime_wide_active_process_count() {
    let mut initial = bootstrap(vec![agent("k7m2", &["/one"])]);
    initial.status.active_process_count = 3;
    let mut app = WatchApp::new(&initial, automatic());
    assert_eq!(app.status_active_process_count, 3);

    app.set_agents(vec![agent("k7m2", &["/one"])]);
    assert_eq!(
        app.status_active_process_count, 3,
        "Agent summaries cannot account for unattributed active processes"
    );

    let effects = app.note_live_event(&HistoryLiveEvent {
        schema_version: HISTORY_LIVE_EVENT_SCHEMA_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        sequence: 2,
        emitted_at_ms: 20,
        event_type: "process_started".to_owned(),
        agent_id: None,
        invocation_id: Some(2),
        presentation_id: Some("inv-2".to_owned()),
        presentation_revision: Some(PRESENTATION_SCHEMA_VERSION),
        payload: json!({"active_process_count": 4}),
    });
    assert_eq!(app.status_active_process_count, 4);
    assert_eq!(effects, vec![WatchEffect::RefreshAgents]);

    app.set_agents(vec![agent("k7m2", &["/one"])]);
    assert_eq!(app.status_active_process_count, 4);
    app.set_status_active_process_count(5);
    assert_eq!(app.status_active_process_count, 5);
}

#[test]
fn waiting_surfaces_unattributed_without_inventing_agent_identity() {
    let mut app = WatchApp::new(&bootstrap(Vec::new()), automatic());
    let effects = app.note_live_event(&HistoryLiveEvent {
        schema_version: HISTORY_LIVE_EVENT_SCHEMA_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        sequence: 1,
        emitted_at_ms: 10,
        event_type: "invocation_started".to_owned(),
        agent_id: None,
        invocation_id: Some(1),
        presentation_id: Some("inv-1".to_owned()),
        presentation_revision: Some(PRESENTATION_SCHEMA_VERSION),
        payload: json!({}),
    });
    assert!(effects.is_empty());
    assert_eq!(app.scope, WatchScope::Unattributed);

    app.merge_detail(command_detail(1, None, "printf hello", "running", None));
    assert_eq!(app.visible_cards().len(), 1);
    assert!(app.visible_cards()[0].record.agent_id.is_none());
}

#[test]
fn waiting_to_multiple_agents_keeps_all_agents_deliberate() {
    let mut app = WatchApp::new(&bootstrap(Vec::new()), automatic());
    assert_eq!(app.scope, WatchScope::Waiting);

    let effects = app.set_agents(vec![agent("k7m2", &["/one"]), agent("m4n8", &["/two"])]);

    assert_eq!(app.scope, WatchScope::Picker);
    assert_eq!(
        app.picker_index, 1,
        "the first Agent is highlighted, not All Agents"
    );
    assert!(
        effects.is_empty(),
        "Waiting and picker both use the same global SSE stream"
    );
}

#[test]
fn waiting_to_one_agent_resubscribes_and_live_handover_is_scoped_and_deduplicated() {
    let mut app = WatchApp::new(&bootstrap(Vec::new()), automatic());
    let effects = app.set_agents(vec![agent("k7m2", &["/one"])]);
    assert_eq!(app.scope, WatchScope::Agent("k7m2".to_owned()));
    assert_eq!(
        effects,
        vec![WatchEffect::Resubscribe(Some("k7m2".to_owned()))]
    );

    let unrelated = HistoryLiveEvent {
        schema_version: HISTORY_LIVE_EVENT_SCHEMA_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        sequence: 10,
        emitted_at_ms: 10,
        event_type: "invocation_started".to_owned(),
        agent_id: Some("m4n8".to_owned()),
        invocation_id: Some(10),
        presentation_id: Some("inv-10".to_owned()),
        presentation_revision: Some(PRESENTATION_SCHEMA_VERSION),
        payload: json!({}),
    };
    assert!(!app.should_process_live_event(&unrelated));

    let selected = HistoryLiveEvent {
        sequence: 9,
        agent_id: Some("k7m2".to_owned()),
        invocation_id: Some(9),
        presentation_id: Some("inv-9".to_owned()),
        ..unrelated
    };
    assert!(app.should_process_live_event(&selected));
    assert!(
        !app.should_process_live_event(&selected),
        "overlapping old/new SSE subscriptions must not duplicate one event"
    );
}

#[test]
fn poll_only_calls_merge_into_one_compact_aggregate() {
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/workspace"])]),
        automatic(),
    );
    for id in 1..=50 {
        app.merge_detail(poll_detail(id, "k7m2", "proc-1"));
    }
    let cards = app.visible_cards();
    assert_eq!(cards.len(), 1);
    match &cards[0].record.kind {
        PresentationKind::PollAggregate {
            count,
            target_session_handle,
            ..
        } => {
            assert_eq!(*count, 50);
            assert_eq!(target_session_handle, "proc-1");
        }
        other => panic!("expected poll aggregate, got {other:?}"),
    }
    assert_eq!(cards[0].record.raw_evidence_count, 50);
    assert_eq!(cards[0].record.raw_invocation_ids.len(), 32);
    assert!(cards[0].record.raw_invocation_ids_truncated);

    for id in 1..=50 {
        app.merge_detail(poll_detail(id, "k7m2", "proc-1"));
    }
    let cards = app.visible_cards();
    assert_eq!(cards.len(), 1);
    match &cards[0].record.kind {
        PresentationKind::PollAggregate { count, .. } => assert_eq!(*count, 50),
        other => panic!("expected poll aggregate, got {other:?}"),
    }
    assert_eq!(
        cards[0].record.raw_evidence_count, 50,
        "re-fetching durable poll details during recovery must be idempotent"
    );
    assert_eq!(cards[0].record.raw_invocation_ids.len(), 32);
}

#[test]
fn canonical_poll_presentation_replaces_live_delta_and_orphan_poll_card() {
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/workspace"])]),
        automatic(),
    );
    app.merge_detail(command_detail(
        1,
        Some("k7m2"),
        "long-task",
        "running",
        Some("start\n"),
    ));
    app.append_live_output(1, "end\n");
    app.merge_detail(poll_detail(2, "k7m2", "proc-1"));
    assert_eq!(app.visible_cards().len(), 2);

    let mut canonical =
        command_detail(1, Some("k7m2"), "long-task", "exited", Some("start\nend\n")).presentation;
    canonical.schema_version = PRESENTATION_SCHEMA_VERSION;
    canonical.records[0].raw_invocation_ids = vec![1, 2];
    canonical.records[0].raw_evidence_count = 2;
    canonical.records[0].raw_invocation_ids_truncated = false;
    if let PresentationKind::Command { polls, .. } = &mut canonical.records[0].kind {
        *polls = Some(PresentationPollSummary {
            count: 1,
            final_status: Some("exited".to_owned()),
            caller_agent_ids: vec!["k7m2".to_owned()],
            cross_agent: false,
        });
    }
    app.merge_presentation(PresentationDocument { ..canonical });

    let cards = app.visible_cards();
    assert_eq!(cards.len(), 1);
    assert!(app.live_output_for(cards[0]).is_none());
    match &cards[0].record.kind {
        PresentationKind::Command {
            status,
            output,
            polls,
            ..
        } => {
            assert_eq!(status, "exited");
            assert_eq!(output.as_deref(), Some("start\nend\n"));
            assert_eq!(polls.as_ref().map(|polls| polls.count), Some(1));
        }
        other => panic!("expected canonical command, got {other:?}"),
    }
}

#[test]
fn updated_poll_aggregate_is_resorted_by_latest_activity() {
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/workspace"])]),
        automatic(),
    );
    app.merge_detail(poll_detail(1, "k7m2", "proc-1"));
    app.merge_detail(command_detail(
        2,
        Some("k7m2"),
        "git status",
        "success",
        Some("clean"),
    ));
    app.merge_detail(poll_detail(3, "k7m2", "proc-1"));

    let cards = app.visible_cards();
    assert_eq!(cards.len(), 2);
    assert!(matches!(
        cards[0].record.kind,
        PresentationKind::Command { .. }
    ));
    assert!(matches!(
        cards[1].record.kind,
        PresentationKind::PollAggregate { .. }
    ));
    assert_eq!(cards[1].record.raw_invocation_ids, vec![1, 3]);
}

#[test]
fn poll_aggregates_remain_scoped_to_the_caller_agent() {
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/one"]), agent("m4n8", &["/two"])]),
        WatchOptions {
            agent: None,
            all: true,
        },
    );
    app.merge_detail(poll_detail(1, "k7m2", "shared-process"));
    app.merge_detail(poll_detail(2, "m4n8", "shared-process"));
    assert_eq!(
        app.visible_cards().len(),
        2,
        "All Agents keeps compact poll aggregates distinct by caller Agent"
    );

    app.cycle_agents_after_refresh(1);
    let first = app.visible_cards();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].record.agent_id.as_deref(), Some("k7m2"));

    app.cycle_agents_after_refresh(1);
    let second = app.visible_cards();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].record.agent_id.as_deref(), Some("m4n8"));
}

#[test]
fn tab_cycles_agents_without_changing_runtime_state() {
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/one"]), agent("m4n8", &["/two"])]),
        WatchOptions {
            agent: None,
            all: true,
        },
    );
    let effects = app.apply_input(WatchInput::CycleAgents(1));
    assert_eq!(effects, vec![WatchEffect::CycleAgents(1)]);
    let effects = app.cycle_agents_after_refresh(1);
    assert_eq!(app.scope, WatchScope::Agent("k7m2".to_owned()));
    assert!(effects.contains(&WatchEffect::Resubscribe(Some("k7m2".to_owned()))));

    let effects = app.cycle_agents_after_refresh(-1);
    assert_eq!(app.scope, WatchScope::Agent("m4n8".to_owned()));
    assert!(effects.contains(&WatchEffect::Resubscribe(Some("m4n8".to_owned()))));
}

#[test]
fn raw_evidence_is_hidden_until_disclosed_and_display_is_terminal_safe() {
    let dangerous = "printf '\u{1b}]8;;https://example.com\u{7}unsafe\u{1b}]8;;\u{7}'";
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/workspace"])]),
        automatic(),
    );
    app.merge_detail(command_detail(1, Some("k7m2"), dangerous, "running", None));
    let card = app.visible_cards()[0];
    let PresentationKind::Command { command, .. } = &card.record.kind else {
        panic!("expected command card");
    };
    assert!(!command.contains('\u{1b}'));
    assert!(!app.raw_is_open(card));
    assert!(app.raw_display_for(card).unwrap().contains("\\u001b"));
    assert!(!app.raw_display_for(card).unwrap().contains('\u{1b}'));

    let default_copy = app.apply_input(WatchInput::Copy);
    assert_eq!(default_copy, vec![WatchEffect::Copy(dangerous.to_owned())]);
    app.apply_input(WatchInput::ToggleRaw);
    let card = app.visible_cards()[0];
    assert!(app.raw_is_open(card));
    let raw_copy = app.apply_input(WatchInput::Copy);
    let WatchEffect::Copy(raw) = &raw_copy[0] else {
        panic!("expected raw copy effect");
    };
    assert!(raw.contains("\\u001b"));
}

#[test]
fn command_card_copy_and_raw_target_parent_invocation_not_folded_poll() {
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/workspace"])]),
        automatic(),
    );
    let mut detail = command_detail(1, Some("k7m2"), "sleep 30", "running", Some("started"));
    detail.presentation.records[0].raw_invocation_ids = vec![1, 2];
    detail.presentation.records[0].raw_evidence_count = 2;
    app.merge_detail(detail);

    assert_eq!(
        app.apply_input(WatchInput::Copy),
        vec![WatchEffect::Copy("sleep 30".to_owned())]
    );
    app.apply_input(WatchInput::ToggleRaw);
    let card = app.visible_cards()[0];
    assert!(app.raw_is_open(card));
    let raw_copy = app.apply_input(WatchInput::Copy);
    let WatchEffect::Copy(raw) = &raw_copy[0] else {
        panic!("expected raw copy effect");
    };
    assert!(raw.contains("\"id\": 1"));
    assert!(raw.contains("sleep 30"));
}

#[test]
fn live_output_is_bounded_and_search_filters_presented_cards() {
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/workspace"])]),
        automatic(),
    );
    app.merge_detail(command_detail(
        1,
        Some("k7m2"),
        "cargo test",
        "running",
        None,
    ));
    app.merge_detail(command_detail(
        2,
        Some("k7m2"),
        "git status",
        "success",
        Some("clean"),
    ));
    app.append_live_output(1, &"x".repeat(40_000));
    let cargo = app.visible_cards()[0];
    let (output, truncated) = app.live_output_for(cargo).unwrap();
    assert!(truncated);
    assert!(output.chars().count() <= 32 * 1024);

    app.append_live_output(1, "\u{1b}]8;;https://example.com\u{7}unsafe\u{1b}]8;;\u{7}");
    let cargo = app.visible_cards()[0];
    let (output, _) = app.live_output_for(cargo).unwrap();
    assert!(!output.contains('\u{1b}'));
    assert!(!output.contains('\u{7}'));

    app.apply_input(WatchInput::StartSearch);
    for ch in "git".chars() {
        app.apply_input(WatchInput::SearchChar(ch));
    }
    app.apply_input(WatchInput::SearchCommit);
    let cards = app.visible_cards();
    assert_eq!(cards.len(), 1);
    match &cards[0].record.kind {
        PresentationKind::Command { command, .. } => assert_eq!(command, "git status"),
        other => panic!("expected command, got {other:?}"),
    }
}

#[test]
fn workdir_and_gap_events_are_visible_without_history_preload() {
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/workspace"])]),
        automatic(),
    );
    let workdir_effects = app.note_live_event(&HistoryLiveEvent {
        schema_version: HISTORY_LIVE_EVENT_SCHEMA_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        sequence: 2,
        emitted_at_ms: 20,
        event_type: "agent_workdir_added".to_owned(),
        agent_id: Some("k7m2".to_owned()),
        invocation_id: Some(2),
        presentation_id: Some("inv-2".to_owned()),
        presentation_revision: Some(PRESENTATION_SCHEMA_VERSION),
        payload: json!({"normalized_workdir": "/workspace/new"}),
    });
    assert_eq!(app.new_workdir_notice(), Some("/workspace/new"));
    assert_eq!(
        app.new_workdir_notice_at(Instant::now() + Duration::from_secs(6)),
        None
    );
    assert_eq!(workdir_effects, vec![WatchEffect::RefreshAgents]);

    let gap_effects = app.note_live_event(&HistoryLiveEvent {
        schema_version: HISTORY_LIVE_EVENT_SCHEMA_VERSION,
        runtime_id: RUNTIME_ID.to_owned(),
        sequence: 12,
        emitted_at_ms: 30,
        event_type: "gap".to_owned(),
        agent_id: Some("k7m2".to_owned()),
        invocation_id: None,
        presentation_id: None,
        presentation_revision: None,
        payload: json!({"skipped_events": 9}),
    });
    assert_eq!(gap_effects, vec![WatchEffect::RefreshAgents]);
    assert!(
        matches!(app.connection, ConnectionState::Degraded(ref message) if message.contains("9 skipped"))
    );

    app.set_recovered("live event gap recovered from durable history");
    assert_eq!(app.connection, ConnectionState::Connected);
    assert_eq!(
        app.recovery_notice(),
        Some("live event gap recovered from durable history")
    );
    assert_eq!(
        app.recovery_notice_at(Instant::now() + Duration::from_secs(6)),
        None
    );
}
