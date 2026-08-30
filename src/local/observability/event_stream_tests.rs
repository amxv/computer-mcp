use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, BodyDataStream};
use axum::http::{HeaderValue, Method, Request, StatusCode, header};
use serde_json::{Value, json};
use tempfile::tempdir;
use tokio_stream::StreamExt as _;
use tower::ServiceExt as _;

use crate::invocation::{
    InvocationContext, InvocationContinuationKind, InvocationEvidenceRecorder, InvocationOutcome,
    InvocationStart, ProviderCallMetadata,
};
use crate::local::history::{HISTORY_LIVE_EVENT_SCHEMA_VERSION, HistoryLiveEvent};
use crate::local::{
    LocalHistoryReader, LocalHistoryRuntime, LocalHistoryRuntimeConfig, PRESENTATION_SCHEMA_VERSION,
};
use crate::protocol::TerminationReason;
use crate::session::{
    OwnedProcess, OwnedProcessEnd, OwnedProcessObserver, ProcessBirthIdentity, ProcessIdentity,
    SessionOutputChunk, SessionOutputCompletion, SessionOutputObserver,
};

use super::server::build_router;

const TOKEN: &str = "observability-event-v2-token-0123456789abcdef";

fn open_history(path: &std::path::Path, runtime_id: &str) -> Arc<LocalHistoryRuntime> {
    LocalHistoryRuntime::open(LocalHistoryRuntimeConfig::new(
        path.to_path_buf(),
        runtime_id,
        60 * 60,
        64 * 1024 * 1024,
    ))
    .unwrap()
}

fn provider_context(session_key: &str) -> InvocationContext {
    InvocationContext::default().with_provider(ProviderCallMetadata::new("openai", session_key))
}

fn complete(history: &LocalHistoryRuntime, context: &InvocationContext, result: serde_json::Value) {
    history
        .complete(context, InvocationOutcome::Success(result))
        .unwrap();
}

async fn receive_event(
    receiver: &mut tokio::sync::broadcast::Receiver<HistoryLiveEvent>,
    event_type: &str,
) -> HistoryLiveEvent {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = receiver.recv().await.unwrap();
            if event.event_type == event_type {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for Local live event")
}

fn router(history: Arc<LocalHistoryRuntime>) -> Router {
    build_router(history, HeaderValue::from_static(TOKEN))
}

async fn request(app: &Router, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(path)
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn next_sse_json(stream: &mut BodyDataStream) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("timed out waiting for SSE frame")
        .expect("SSE stream ended unexpectedly")
        .expect("failed to read SSE frame");
    let text = String::from_utf8(frame.to_vec()).unwrap();
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap_or_else(|| panic!("SSE frame was missing data: {text}"));
    serde_json::from_str(data).unwrap()
}

async fn assert_no_output_event_within(stream: &mut BodyDataStream, duration: Duration) {
    let deadline = tokio::time::Instant::now() + duration;
    loop {
        let frame = match tokio::time::timeout_at(deadline, stream.next()).await {
            Err(_) => return,
            Ok(None) => return,
            Ok(Some(frame)) => frame.expect("failed to read SSE frame"),
        };
        let text = String::from_utf8(frame.to_vec()).unwrap();
        let data = text
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap_or_else(|| panic!("SSE frame was missing data: {text}"));
        let event: Value = serde_json::from_str(data).unwrap();
        assert!(
            !matches!(
                event["event_type"].as_str(),
                Some("output" | "output_complete")
            ),
            "output selection leaked a PTY event: {event}"
        );
    }
}

fn output_chunk(event: &Value, sequence: u64) -> Option<&Value> {
    (event["event_type"] == "output")
        .then(|| event["payload"]["chunks"].as_array())
        .flatten()?
        .iter()
        .find(|chunk| chunk["sequence"] == sequence)
}

#[tokio::test]
async fn canonical_event_identity_maps_polls_to_parent_and_process_end_refreshes_command() {
    let dir = tempdir().unwrap();
    let database = dir.path().join("history.sqlite3");
    let history = open_history(&database, "runtime-event-identity");
    let (_sequence, mut events) = history.subscribe_live_events();

    let command = history
        .begin(
            provider_context("identity-provider"),
            InvocationStart::new("exec_command", json!({"cmd":"sleep 30"})),
        )
        .unwrap();
    let command_id = command.invocation_id.unwrap();
    let command_presentation_id = format!("inv-{command_id}");
    let started = receive_event(&mut events, "invocation_started").await;
    assert_eq!(started.schema_version, HISTORY_LIVE_EVENT_SCHEMA_VERSION);
    assert_eq!(started.invocation_id, Some(command_id));
    assert_eq!(
        started.presentation_id.as_deref(),
        Some(command_presentation_id.as_str())
    );
    assert_eq!(
        started.presentation_revision,
        Some(PRESENTATION_SCHEMA_VERSION)
    );

    let process = OwnedProcess {
        internal_session_id: 501,
        session_handle: Arc::from("identity-handle"),
        identity: ProcessIdentity {
            pid: 50_001,
            birth: ProcessBirthIdentity::LinuxProcStartTicks { ticks: 501 },
        },
        created_by: command.clone(),
    };
    history.process_started(&process).unwrap();
    let process_started = receive_event(&mut events, "process_started").await;
    assert_eq!(
        process_started.presentation_id.as_deref(),
        Some(command_presentation_id.as_str())
    );

    complete(
        &history,
        &command,
        json!({
            "status":"running",
            "session_handle":"identity-handle",
            "output":""
        }),
    );
    history.flush_for_test().unwrap();
    let completed = receive_event(&mut events, "invocation_completed").await;
    assert_eq!(completed.invocation_id, Some(command_id));
    assert_eq!(
        completed.presentation_id.as_deref(),
        Some(command_presentation_id.as_str())
    );
    let updated = receive_event(&mut events, "presentation_updated").await;
    assert_eq!(
        updated.presentation_id.as_deref(),
        Some(command_presentation_id.as_str())
    );

    let poll = history
        .begin(
            provider_context("identity-provider"),
            InvocationStart::new(
                "write_stdin",
                json!({"session_handle":"identity-handle","chars":"","kill_process":false}),
            )
            .with_target_created_by_agent_id(command.agent_id.clone())
            .with_target_created_by_invocation_id(Some(command_id))
            .with_continuation_kind(InvocationContinuationKind::Poll),
        )
        .unwrap();
    let poll_id = poll.invocation_id.unwrap();
    let poll_started = receive_event(&mut events, "invocation_started").await;
    assert_eq!(poll_started.invocation_id, Some(poll_id));
    assert_eq!(
        poll_started.presentation_id.as_deref(),
        Some(command_presentation_id.as_str()),
        "no-input polls must name their owning command card"
    );
    let poll_started_update = receive_event(&mut events, "presentation_updated").await;
    assert_eq!(
        poll_started_update.presentation_id.as_deref(),
        Some(command_presentation_id.as_str())
    );
    assert_eq!(poll_started_update.payload["source"], "invocation_started");
    let poll_started_record = LocalHistoryReader::timeline_detail(&database, command_id)
        .unwrap()
        .unwrap();
    match poll_started_record.kind {
        crate::local::PresentationKind::Command { polls, .. } => {
            assert_eq!(
                polls.unwrap().count,
                1,
                "poll count must advance on poll start"
            );
        }
        other => panic!("expected command timeline record, got {other:?}"),
    }
    complete(&history, &poll, json!({"status":"running"}));
    history.flush_for_test().unwrap();
    let poll_completed = receive_event(&mut events, "invocation_completed").await;
    assert_eq!(poll_completed.invocation_id, Some(poll_id));
    assert_eq!(
        poll_completed.presentation_id.as_deref(),
        Some(command_presentation_id.as_str())
    );
    let poll_updated = receive_event(&mut events, "presentation_updated").await;
    assert_eq!(
        poll_updated.presentation_id.as_deref(),
        Some(command_presentation_id.as_str())
    );

    let stdin = history
        .begin(
            provider_context("identity-provider"),
            InvocationStart::new(
                "write_stdin",
                json!({"session_handle":"identity-handle","chars":"y\n","kill_process":false}),
            )
            .with_target_created_by_agent_id(command.agent_id.clone())
            .with_target_created_by_invocation_id(Some(command_id))
            .with_continuation_kind(InvocationContinuationKind::Stdin),
        )
        .unwrap();
    let stdin_id = stdin.invocation_id.unwrap();
    let stdin_started = receive_event(&mut events, "invocation_started").await;
    assert_eq!(stdin_started.invocation_id, Some(stdin_id));
    assert_eq!(
        stdin_started.presentation_id.as_deref(),
        Some(format!("inv-{stdin_id}").as_str()),
        "real stdin remains its own canonical card"
    );
    complete(&history, &stdin, json!({"status":"running"}));
    history.flush_for_test().unwrap();
    let _ = receive_event(&mut events, "invocation_completed").await;
    let _ = receive_event(&mut events, "presentation_updated").await;

    history
        .process_ended(
            &process,
            &OwnedProcessEnd::exited(0, TerminationReason::Exit, "/tmp".to_string()),
        )
        .unwrap();
    let process_ended = receive_event(&mut events, "process_ended").await;
    assert_eq!(
        process_ended.presentation_id.as_deref(),
        Some(command_presentation_id.as_str())
    );
    let final_refresh = receive_event(&mut events, "presentation_updated").await;
    assert_eq!(
        final_refresh.presentation_id.as_deref(),
        Some(command_presentation_id.as_str())
    );
    assert_eq!(final_refresh.payload["source"], "process_ended");

    let record = LocalHistoryReader::timeline_detail(&database, command_id)
        .unwrap()
        .unwrap();
    match record.kind {
        crate::local::PresentationKind::Command {
            status, exit_code, ..
        } => {
            assert_eq!(status, "exited");
            assert_eq!(exit_code, Some(0));
        }
        other => panic!("expected command timeline record, got {other:?}"),
    }

    history.observe_output_complete(SessionOutputCompletion {
        internal_session_id: 501,
        session_handle: Arc::from("identity-handle"),
        invocation: command,
    });
    history.flush_for_test().unwrap();
    history.shutdown_blocking().unwrap();
}

#[tokio::test]
async fn output_bursts_are_batched_so_terminal_control_events_do_not_overrun_the_sse_ring() {
    let dir = tempdir().unwrap();
    let database = dir.path().join("history.sqlite3");
    let history = LocalHistoryRuntime::open(
        LocalHistoryRuntimeConfig::new(
            database,
            "runtime-output-batch-ring",
            60 * 60,
            64 * 1024 * 1024,
        )
        .with_event_capacity(3),
    )
    .unwrap();
    let command = history
        .begin(
            provider_context("output-batch-ring"),
            InvocationStart::new("exec_command", json!({"cmd":"noisy"})),
        )
        .unwrap();
    let invocation_id = command.invocation_id.unwrap();
    let process = OwnedProcess {
        internal_session_id: 777,
        session_handle: Arc::from("output-batch-ring"),
        identity: ProcessIdentity {
            pid: 77_777,
            birth: ProcessBirthIdentity::LinuxProcStartTicks { ticks: 777 },
        },
        created_by: command.clone(),
    };
    history.process_started(&process).unwrap();
    complete(
        &history,
        &command,
        json!({"status":"running","session_handle":"output-batch-ring"}),
    );
    history.flush_for_test().unwrap();

    let (_sequence, mut events) = history.subscribe_live_events();
    let mut output_events = history.subscribe_live_output_events();
    for sequence in 0..64 {
        history.observe_output(SessionOutputChunk {
            internal_session_id: 777,
            session_handle: Arc::from("output-batch-ring"),
            invocation: command.clone(),
            sequence,
            text: format!("chunk-{sequence}\n"),
        });
    }
    history
        .process_ended(
            &process,
            &OwnedProcessEnd::exited(0, TerminationReason::Exit, "/tmp".to_string()),
        )
        .unwrap();
    history.flush_for_test().unwrap();

    let mut seen = Vec::new();
    for _ in 0..2 {
        seen.push(
            tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .expect("timed out waiting for batched live event")
                .expect("PTY output must not lag the three-slot control-event ring"),
        );
    }
    assert!(seen.iter().any(|event| event.event_type == "process_ended"));
    assert!(
        seen.iter()
            .any(|event| event.event_type == "presentation_updated")
    );
    let mut observed_chunks = 0;
    while observed_chunks < 64 {
        let output = tokio::time::timeout(Duration::from_secs(2), output_events.recv())
            .await
            .expect("timed out waiting for output event")
            .expect("output burst should remain recoverable on its independent channel");
        assert_eq!(output.event_type, "output");
        assert_eq!(output.sequence, 0);
        assert_eq!(output.invocation_id, Some(invocation_id));
        observed_chunks += output.payload["chunks"].as_array().unwrap().len();
    }
    assert_eq!(observed_chunks, 64);

    drop(events);
    drop(output_events);
    history.observe_output_complete(SessionOutputCompletion {
        internal_session_id: 777,
        session_handle: Arc::from("output-batch-ring"),
        invocation: command,
    });
    history.flush_for_test().unwrap();
    history.shutdown_blocking().unwrap();
}

#[tokio::test]
async fn public_sse_keeps_global_metadata_but_selects_output_and_preserves_streaming_parser_state()
{
    let dir = tempdir().unwrap();
    let database = dir.path().join("history.sqlite3");
    let history = open_history(&database, "runtime-output-selection");

    let command_a = history
        .begin(
            provider_context("selection-provider-a"),
            InvocationStart::new("exec_command", json!({"cmd":"agent-a"})),
        )
        .unwrap();
    let command_b = history
        .begin(
            provider_context("selection-provider-b"),
            InvocationStart::new("exec_command", json!({"cmd":"agent-b"})),
        )
        .unwrap();
    let agent_a = command_a.agent_id.clone().unwrap().to_string();
    let agent_b = command_b.agent_id.clone().unwrap().to_string();
    let command_a_id = command_a.invocation_id.unwrap();
    let command_b_id = command_b.invocation_id.unwrap();
    assert_ne!(agent_a, agent_b);

    history.observe_output(SessionOutputChunk {
        internal_session_id: 601,
        session_handle: Arc::from("selection-a"),
        invocation: command_a.clone(),
        sequence: 0,
        text: "before \u{1b}[31".to_string(),
    });
    history.flush_for_test().unwrap();
    assert_eq!(
        history.live_event_sequence(),
        0,
        "the parser must advance even though no live event subscriber existed"
    );

    let app = router(history.clone());
    let selected_response = request(&app, &format!("/v1/events?output_agent_ids={agent_a}")).await;
    assert_eq!(selected_response.status(), StatusCode::OK);
    let mut selected = selected_response.into_body().into_data_stream();

    let metadata_a = history
        .begin(
            provider_context("selection-provider-a"),
            InvocationStart::new("read_file", json!({"path":"a"})),
        )
        .unwrap();
    let metadata_b = history
        .begin(
            provider_context("selection-provider-b"),
            InvocationStart::new("read_file", json!({"path":"b"})),
        )
        .unwrap();
    history.observe_output(SessionOutputChunk {
        internal_session_id: 601,
        session_handle: Arc::from("selection-a"),
        invocation: command_a.clone(),
        sequence: 1,
        text: "mred\u{1b}[0m after".to_string(),
    });
    history.observe_output(SessionOutputChunk {
        internal_session_id: 602,
        session_handle: Arc::from("selection-b"),
        invocation: command_b.clone(),
        sequence: 0,
        text: "blue".to_string(),
    });
    history.flush_for_test().unwrap();

    let mut frames = Vec::new();
    while frames.len() < 8
        && !(frames.iter().any(|event: &Value| {
            event["event_type"] == "invocation_started"
                && event["invocation_id"] == metadata_a.invocation_id.unwrap()
        }) && frames.iter().any(|event: &Value| {
            event["event_type"] == "invocation_started"
                && event["invocation_id"] == metadata_b.invocation_id.unwrap()
        }) && frames
            .iter()
            .any(|event: &Value| output_chunk(event, 1).is_some()))
    {
        frames.push(next_sse_json(&mut selected).await);
    }
    assert!(frames.iter().any(|event| {
        event["event_type"] == "invocation_started"
            && event["agent_id"] == agent_a
            && event["invocation_id"] == metadata_a.invocation_id.unwrap()
    }));
    assert!(frames.iter().any(|event| {
        event["event_type"] == "invocation_started"
            && event["agent_id"] == agent_b
            && event["invocation_id"] == metadata_b.invocation_id.unwrap()
    }));
    assert!(
        frames
            .iter()
            .filter(|event| event["event_type"] == "output")
            .all(|event| event["agent_id"] == agent_a),
        "an A-only output subscription must never leak Agent B output: {frames:?}"
    );
    let output = frames
        .iter()
        .find(|event| output_chunk(event, 1).is_some())
        .unwrap();
    let selected_chunk = output_chunk(output, 1).unwrap();
    assert_eq!(output["agent_id"], agent_a);
    assert_eq!(output["invocation_id"], command_a_id);
    assert_eq!(output["presentation_id"], format!("inv-{command_a_id}"));
    assert_eq!(selected_chunk["sequence"], 1);
    assert_eq!(selected_chunk["text"], "red after");
    assert_eq!(output["payload"]["display_state"], "available");
    assert!(!output.to_string().contains("\\u001b"));

    history.observe_output(SessionOutputChunk {
        internal_session_id: 601,
        session_handle: Arc::from("selection-a"),
        invocation: command_a.clone(),
        sequence: 3,
        text: "uncertain".to_string(),
    });
    history.flush_for_test().unwrap();
    let degraded = loop {
        let event = next_sse_json(&mut selected).await;
        if event["event_type"] == "output" {
            assert_eq!(event["agent_id"], agent_a);
            if output_chunk(&event, 3).is_some() {
                break event;
            }
        }
    };
    let degraded_chunk = output_chunk(&degraded, 3).unwrap();
    assert_eq!(degraded["event_type"], "output");
    assert_eq!(degraded_chunk["sequence"], 3);
    assert_eq!(degraded_chunk["text"], "");
    assert_eq!(degraded["payload"]["display_state"], "unavailable");
    assert!(
        degraded["payload"]["display_reason"]
            .as_str()
            .unwrap()
            .contains("sequence")
    );

    history.observe_output_complete(SessionOutputCompletion {
        internal_session_id: 601,
        session_handle: Arc::from("selection-a"),
        invocation: command_a.clone(),
    });
    history.observe_output_complete(SessionOutputCompletion {
        internal_session_id: 602,
        session_handle: Arc::from("selection-b"),
        invocation: command_b.clone(),
    });
    history.flush_for_test().unwrap();
    let completed = loop {
        let event = next_sse_json(&mut selected).await;
        if event["event_type"] == "output_complete" {
            break event;
        }
    };
    assert_eq!(completed["event_type"], "output_complete");
    assert_eq!(completed["invocation_id"], command_a_id);
    assert_eq!(completed["payload"]["display_state"], "unavailable");

    let empty_response = request(&app, "/v1/events?output_agent_ids=").await;
    assert_eq!(empty_response.status(), StatusCode::OK);
    let mut empty = empty_response.into_body().into_data_stream();
    let metadata_empty = history
        .begin(
            provider_context("selection-provider-b"),
            InvocationStart::new("read_file", json!({"path":"empty-selection"})),
        )
        .unwrap();
    history.observe_output(SessionOutputChunk {
        internal_session_id: 603,
        session_handle: Arc::from("selection-empty"),
        invocation: metadata_empty.clone(),
        sequence: 0,
        text: "must-not-arrive".to_string(),
    });
    history.flush_for_test().unwrap();
    let empty_metadata = next_sse_json(&mut empty).await;
    assert_eq!(empty_metadata["event_type"], "invocation_started");
    assert_eq!(
        empty_metadata["invocation_id"],
        metadata_empty.invocation_id.unwrap()
    );
    assert_no_output_event_within(&mut empty, Duration::from_millis(100)).await;

    let no_output_response = request(&app, "/v1/events?include_output=false").await;
    assert_eq!(no_output_response.status(), StatusCode::OK);
    let mut no_output = no_output_response.into_body().into_data_stream();
    let metadata_no_output = history
        .begin(
            provider_context("selection-provider-a"),
            InvocationStart::new("read_file", json!({"path":"no-output"})),
        )
        .unwrap();
    history.observe_output(SessionOutputChunk {
        internal_session_id: 604,
        session_handle: Arc::from("selection-none"),
        invocation: metadata_no_output.clone(),
        sequence: 0,
        text: "also-must-not-arrive".to_string(),
    });
    history.flush_for_test().unwrap();
    let no_output_metadata = next_sse_json(&mut no_output).await;
    assert_eq!(no_output_metadata["event_type"], "invocation_started");
    assert_eq!(
        no_output_metadata["invocation_id"],
        metadata_no_output.invocation_id.unwrap()
    );
    assert_no_output_event_within(&mut no_output, Duration::from_millis(100)).await;

    let too_many = (0..33)
        .map(|index| format!("a{index:03}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        request(&app, &format!("/v1/events?output_agent_ids={too_many}"))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&app, "/v1/events?output_agent_ids=bad")
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );

    complete(&history, &metadata_a, json!({"ok":true}));
    complete(&history, &metadata_b, json!({"ok":true}));
    complete(&history, &metadata_empty, json!({"ok":true}));
    complete(&history, &metadata_no_output, json!({"ok":true}));
    complete(&history, &command_a, json!({"status":"exited"}));
    complete(&history, &command_b, json!({"status":"exited"}));
    history.flush_for_test().unwrap();
    drop(selected);
    drop(empty);
    drop(no_output);
    drop(app);
    history.shutdown_blocking().unwrap();
    let _ = command_b_id;
}
