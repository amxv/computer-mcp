use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::json;

use super::super::history::{HISTORY_LIVE_EVENT_SCHEMA_VERSION, HistoryLiveEvent};
use super::super::{
    PRESENTATION_SCHEMA_VERSION, PresentationDiffLine, PresentationFileChange,
    PresentationFileOperation, PresentationKind, PresentationWriteMode,
};
use super::input::WatchInput;
use super::model::{WatchApp, WatchOptions};
use super::render::render;
use super::test_support::{
    RUNTIME_ID, agent, bootstrap, command_detail, detail, poll_detail, record,
};

fn render_text(app: &WatchApp, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    let mut text = String::new();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = buffer.cell((x, y)) {
                text.push_str(cell.symbol());
            }
        }
        text.push('\n');
    }
    text
}

fn direct_app() -> WatchApp {
    WatchApp::new(
        &bootstrap(vec![agent(
            "k7m2",
            &["/workspace", "/workspace/really-long-second-workdir"],
        )]),
        WatchOptions::automatic(),
    )
}

#[test]
fn renders_waiting_one_agent_and_multi_agent_picker_modes() {
    let waiting = WatchApp::new(&bootstrap(Vec::new()), WatchOptions::automatic());
    let waiting_text = render_text(&waiting, 80, 18);
    assert!(waiting_text.contains("Waiting for Agent"));
    assert!(waiting_text.contains("completed history is not preloaded"));

    let direct = direct_app();
    let direct_text = render_text(&direct, 100, 20);
    assert!(direct_text.contains("Agent k7m2"));
    assert!(direct_text.contains("/workspace"));
    assert!(direct_text.contains("processes: 1"));
    assert!(direct_text.contains("TTL:"));

    let picker = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &["/one"]), agent("m4n8", &["/two"])]),
        WatchOptions::automatic(),
    );
    let picker_text = render_text(&picker, 90, 20);
    assert!(picker_text.contains("Choose Agent"));
    assert!(picker_text.contains("All Agents"));
    assert!(picker_text.contains("k7m2"));
    assert!(picker_text.contains("m4n8"));
}

#[test]
fn renders_explicit_unattributed_activity_instead_of_idle_waiting() {
    let mut app = WatchApp::new(&bootstrap(Vec::new()), WatchOptions::automatic());
    app.note_live_event(&HistoryLiveEvent {
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
    let text = render_text(&app, 90, 18);
    assert!(text.contains("Unattributed Local activity was observed"));
    assert!(text.contains("No Agent identity is being inferred"));
    assert!(!text.contains("Waiting for the first current-runtime Agent"));
}

#[test]
fn dedicated_missing_agent_renders_waiting_state_without_fabricating_presence() {
    let app = WatchApp::new(
        &bootstrap(Vec::new()),
        WatchOptions {
            agent: Some("k7m2".to_owned()),
            all: false,
        },
    );
    let text = render_text(&app, 80, 18);
    assert!(text.contains("Agent k7m2"));
    assert!(text.contains("Waiting for Agent k7m2"));
    assert!(text.contains("completed history is not preloaded"));
}

#[test]
fn renders_compact_running_completed_failed_and_all_agent_command_cards() {
    let mut app = WatchApp::new(
        &bootstrap(vec![
            agent("k7m2", &["/workspace"]),
            agent("m4n8", &["/other"]),
        ]),
        WatchOptions {
            agent: None,
            all: true,
        },
    );
    app.merge_detail(command_detail(
        1,
        Some("k7m2"),
        "cargo test",
        "running",
        Some("Compiling zodex"),
    ));
    app.merge_detail(command_detail(
        2,
        Some("m4n8"),
        "git status",
        "success",
        Some("clean"),
    ));
    app.merge_detail(command_detail(
        3,
        Some("k7m2"),
        "false",
        "error",
        Some("failed"),
    ));
    let text = render_text(&app, 100, 28);
    assert!(text.contains("[k7m2] … $ cargo test"));
    assert!(text.contains("Compiling zodex"));
    assert!(text.contains("[m4n8] ✓ $ git status"));
    assert!(text.contains("[k7m2] ✗ $ false"));
    assert!(text.contains("→ cwd /workspace/subdir"));
}

#[test]
fn incomplete_command_status_is_not_rendered_as_success() {
    let mut app = direct_app();
    app.merge_detail(command_detail(
        7,
        Some("k7m2"),
        "interrupted command",
        "incomplete",
        None,
    ));
    let text = render_text(&app, 90, 20);
    assert!(text.contains("? $ interrupted command"));
    assert!(!text.contains("✓ $ interrupted command"));
}

#[test]
fn renders_structured_file_diff_and_write_mode() {
    let mut app = direct_app();
    let file = PresentationFileChange {
        operation: PresentationFileOperation::Edited,
        path: "src/main.rs".to_owned(),
        old_path: None,
        write_mode: Some(PresentationWriteMode::Overwrite),
        added: 1,
        removed: 1,
        diff_truncated: false,
        diff_lines_included: true,
        lines: vec![
            PresentationDiffLine {
                kind: "remove".to_owned(),
                old_line: Some(1),
                new_line: None,
                text: "old".to_owned(),
            },
            PresentationDiffLine {
                kind: "add".to_owned(),
                old_line: None,
                new_line: Some(1),
                text: "new".to_owned(),
            },
        ],
    };
    app.merge_detail(detail(
        4,
        Some("k7m2"),
        "apply_patch",
        json!({"patch": "*** Begin Patch"}),
        record(
            4,
            Some("k7m2"),
            PresentationKind::FileChanges {
                source_tool: "apply_patch".to_owned(),
                changes: vec![file],
            },
        ),
    ));
    let compact = render_text(&app, 90, 20);
    assert!(compact.contains("Edited src/main.rs  +1 -1 · overwrite"));

    app.apply_input(WatchInput::Enter);
    let expanded = render_text(&app, 90, 20);
    assert!(expanded.contains("--- src/main.rs"));
    assert!(expanded.contains("-old"));
    assert!(expanded.contains("+new"));
}

#[test]
fn renders_one_poll_aggregate_and_cross_agent_stdin() {
    let mut app = direct_app();
    for id in 10..60 {
        app.merge_detail(poll_detail(id, "k7m2", "proc-1"));
    }
    app.merge_detail(detail(
        61,
        Some("k7m2"),
        "write_stdin",
        json!({"session_handle": "proc-2", "chars": "yes\n"}),
        record(
            61,
            Some("k7m2"),
            PresentationKind::Stdin {
                target_session_handle: "proc-2".to_owned(),
                chars: "yes\n".to_owned(),
                chars_truncated: false,
                creator_agent_id: Some("m4n8".to_owned()),
                cross_agent: true,
                result_status: Some("success".to_owned()),
            },
        ),
    ));
    app.merge_detail(detail(
        62,
        Some("k7m2"),
        "kill_session",
        json!({"session_handle": "proc-3"}),
        record(
            62,
            Some("k7m2"),
            PresentationKind::Kill {
                target_session_handle: "proc-3".to_owned(),
                target_command: Some("cargo test --workspace".to_owned()),
                creator_agent_id: Some("m4n8".to_owned()),
                cross_agent: true,
                result_status: Some("success".to_owned()),
            },
        ),
    ));
    let text = render_text(&app, 100, 22);
    assert_eq!(text.matches("poll proc-1 ×50").count(), 1);
    assert!(text.contains("stdin proc-2"));
    assert!(text.contains("kill cargo test --workspace"));
    assert!(text.contains("cross-Agent"));
    assert!(text.contains("creator m4n8"));
}

#[test]
fn raw_control_sequences_are_never_replayed_to_test_backend() {
    let dangerous = "printf '\u{1b}[31mred\u{1b}[0m\u{1b}]8;;https://x\u{7}link'";
    let mut app = direct_app();
    app.merge_detail(command_detail(1, Some("k7m2"), dangerous, "running", None));
    app.apply_input(WatchInput::ToggleRaw);
    let text = render_text(&app, 100, 25);
    assert!(text.contains("RAW LOGICAL EVIDENCE"));
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("\\u001b"));
}

#[test]
fn agent_workdir_controls_are_sanitized_and_degraded_state_remains_visible() {
    let dangerous_workdir = "/workspace/\u{1b}]8;;https://x\u{7}unsafe";
    let mut app = WatchApp::new(
        &bootstrap(vec![agent("k7m2", &[dangerous_workdir])]),
        WatchOptions::automatic(),
    );
    app.set_degraded("live stream disconnected");

    let text = render_text(&app, 100, 20);
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("/workspace/unsafe"));
    assert!(text.contains("live stream disconnected"));
}

#[test]
fn small_terminal_and_long_output_scroll_without_panicking() {
    let mut app = direct_app();
    let output = (0..120)
        .map(|index| format!("line-{index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.merge_detail(command_detail(
        1,
        Some("k7m2"),
        "long command",
        "running",
        Some(&output),
    ));
    let small = render_text(&app, 34, 10);
    assert!(small.contains("Zodex Local"));

    app.apply_input(WatchInput::Enter);
    let top = render_text(&app, 50, 14);
    app.apply_input(WatchInput::Move(30));
    let scrolled = render_text(&app, 50, 14);
    assert_ne!(top, scrolled);
    assert!(scrolled.contains("line-"));
}
