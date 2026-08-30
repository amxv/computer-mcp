use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderValue, Method, Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::tempdir;
use tower::ServiceExt as _;

use crate::invocation::{
    InvocationContext, InvocationContinuationKind, InvocationEvidenceRecorder, InvocationOutcome,
    InvocationStart, ProviderCallMetadata,
};
use crate::local::{LocalHistoryRuntime, LocalHistoryRuntimeConfig};
use crate::protocol::TerminationReason;
use crate::session::{
    OwnedProcess, OwnedProcessEnd, OwnedProcessObserver, ProcessBirthIdentity, ProcessIdentity,
    SessionOutputChunk, SessionOutputCompletion, SessionOutputObserver,
};

use super::server::build_router;

const TOKEN: &str = "timeline-observability-token-0123456789abcdef";

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

fn router(history: Arc<LocalHistoryRuntime>) -> Router {
    build_router(history, HeaderValue::from_static(TOKEN))
}

async fn request(app: &Router, path: &str, token: Option<&str>) -> axum::response::Response {
    let mut builder = Request::builder().method(Method::GET).uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn timeline_routes_are_lazy_filtered_cursor_paginated_and_auditable() {
    let dir = tempdir().unwrap();
    let database = dir.path().join("history.sqlite3");
    let workdir_a = dir.path().join("repo-a");
    let workdir_b = dir.path().join("repo-b");
    std::fs::create_dir_all(&workdir_a).unwrap();
    std::fs::create_dir_all(&workdir_b).unwrap();
    let history = open_history(&database, "runtime-timeline-api");

    let command = history
        .begin(
            provider_context("provider-secret-api-a"),
            InvocationStart::new(
                "exec_command",
                json!({
                    "cmd": "long-running-test",
                    "workdir": workdir_a.display().to_string()
                }),
            ),
        )
        .unwrap();
    let command_id = command.invocation_id.unwrap();
    let agent_a = command.agent_id.clone().unwrap();
    complete(
        &history,
        &command,
        json!({
            "status":"running",
            "session_handle":"timeline-api-handle",
            "output":"COMMAND-EXACT-SECRET"
        }),
    );

    let mut poll_ids = Vec::new();
    for index in 0..12 {
        let poll = history
            .begin(
                provider_context("provider-secret-api-a"),
                InvocationStart::new(
                    "write_stdin",
                    json!({
                        "session_handle":"timeline-api-handle",
                        "chars":"",
                        "kill_process":false
                    }),
                )
                .with_target_created_by_agent_id(Some(agent_a.clone()))
                .with_target_created_by_invocation_id(Some(command_id))
                .with_continuation_kind(InvocationContinuationKind::Poll),
            )
            .unwrap();
        poll_ids.push(poll.invocation_id.unwrap());
        complete(
            &history,
            &poll,
            json!({
                "status": if index == 11 { "exited" } else { "running" },
                "exit_code": if index == 11 { Some(0) } else { None::<i64> },
                "output": format!("POLL-EXACT-SECRET-{index}")
            }),
        );
    }

    let generic_a = history
        .begin(
            provider_context("provider-secret-api-a"),
            InvocationStart::new(
                "read_file",
                json!({"workdir":workdir_a.display().to_string(),"path":"a.txt"}),
            ),
        )
        .unwrap();
    complete(
        &history,
        &generic_a,
        json!({"summary":"GENERIC-A-EXACT-SECRET"}),
    );
    let generic_b = history
        .begin(
            provider_context("provider-secret-api-b"),
            InvocationStart::new(
                "read_file",
                json!({"workdir":workdir_b.display().to_string(),"path":"b.txt"}),
            ),
        )
        .unwrap();
    complete(
        &history,
        &generic_b,
        json!({"summary":"GENERIC-B-EXACT-SECRET"}),
    );
    history.flush_for_test().unwrap();
    let app = router(history.clone());

    let unauthorized = request(&app, "/v1/timeline", None).await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        unauthorized.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );

    let first_response = request(&app, "/v1/timeline?limit=2", Some(TOKEN)).await;
    assert_eq!(first_response.status(), StatusCode::OK);
    assert_eq!(
        first_response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    let first = body_json(first_response).await;
    assert_eq!(first["schema_version"], 1);
    assert_eq!(first["presentation_version"], 3);
    assert_eq!(first["records"].as_array().unwrap().len(), 2);
    assert_eq!(first["has_more"], true);
    let cursor = first["next_cursor"].as_str().unwrap();
    assert!(!cursor.contains("started_at_ms"));
    let first_ids = first["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["primary_invocation_id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    let second = body_json(
        request(
            &app,
            &format!("/v1/timeline?limit=2&cursor={cursor}"),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    let second_ids = second["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["primary_invocation_id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert!(first_ids.iter().all(|id| !second_ids.contains(id)));

    let filtered = body_json(
        request(
            &app,
            &format!("/v1/timeline?limit=20&agent_id={agent_a}"),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert_eq!(filtered["records"].as_array().unwrap().len(), 2);
    assert!(
        filtered["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| { record["agent_id"].as_str() == Some(agent_a.as_ref()) })
    );
    let filtered_text = filtered.to_string();
    assert!(!filtered_text.contains("provider-secret-api-a"));
    assert!(!filtered_text.contains("COMMAND-EXACT-SECRET"));
    assert!(!filtered_text.contains("POLL-EXACT-SECRET"));
    assert!(!filtered_text.contains("GENERIC-A-EXACT-SECRET"));

    let workdir_filtered = body_json(
        request(
            &app,
            &format!("/v1/timeline?limit=20&workdir={}", workdir_b.display()),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert_eq!(workdir_filtered["records"].as_array().unwrap().len(), 1);
    assert_eq!(
        workdir_filtered["records"][0]["primary_invocation_id"],
        generic_b.invocation_id.unwrap()
    );

    let detail =
        body_json(request(&app, &format!("/v1/timeline/inv-{command_id}"), Some(TOKEN)).await)
            .await;
    assert_eq!(detail["record"]["primary_invocation_id"], command_id);
    assert_eq!(detail["record"]["raw_evidence_count"], 13);
    assert_eq!(detail["record"]["polls"]["count"], 12);
    assert!(!detail.to_string().contains("POLL-EXACT-SECRET"));

    let batch = body_json(
        request(
            &app,
            &format!(
                "/v1/timeline/diffs?presentation_ids=inv-{command_id},inv-{}",
                generic_b.invocation_id.unwrap()
            ),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert_eq!(batch["presentation_version"], 3);
    assert_eq!(batch["records"].as_array().unwrap().len(), 2);

    let checkpoints = body_json(
        request(
            &app,
            &format!("/v1/timeline/inv-{command_id}/checkpoints?limit=5"),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert_eq!(checkpoints["checkpoints"].as_array().unwrap().len(), 5);
    assert_eq!(checkpoints["has_more"], true);
    assert_eq!(checkpoints["checkpoints"][0]["invocation_id"], command_id);
    assert!(!checkpoints.to_string().contains("POLL-EXACT-SECRET"));

    let exact_poll = body_json(
        request(
            &app,
            &format!("/v1/invocations/{}", poll_ids[0]),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert!(
        exact_poll.to_string().contains("POLL-EXACT-SECRET-0"),
        "exact invocation audit remains available only when explicitly opened"
    );

    let malformed = request(&app, "/v1/timeline?cursor=not_base64!!", Some(TOKEN)).await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    let bad_projection = request(&app, "/v1/timeline?diffs=maybe", Some(TOKEN)).await;
    assert_eq!(bad_projection.status(), StatusCode::BAD_REQUEST);
    let newer_cursor = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({
            "version": 2,
            "kind": "history",
            "timestamp_ms": 1,
            "id": 1,
            "scope_ms": null,
            "scope_id": null
        }))
        .unwrap(),
    );
    let newer = request(
        &app,
        &format!("/v1/timeline?cursor={newer_cursor}"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(newer.status(), StatusCode::BAD_REQUEST);
    let wrong_mode = request(
        &app,
        &format!("/v1/timeline?recovery_since_ms=0&cursor={cursor}"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(wrong_mode.status(), StatusCode::BAD_REQUEST);
    let bad_id = request(&app, "/v1/timeline/not-an-id", Some(TOKEN)).await;
    assert_eq!(bad_id.status(), StatusCode::BAD_REQUEST);
    let missing = request(&app, "/v1/timeline/inv-999999999", Some(TOKEN)).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    drop(app);
    history.shutdown_blocking().unwrap();
}

#[tokio::test]
async fn output_display_view_is_stateful_keeps_raw_default_and_exposes_tail_anchor() {
    let dir = tempdir().unwrap();
    let database = dir.path().join("history.sqlite3");
    let history = open_history(&database, "runtime-output-display-api");
    let command = history
        .begin(
            provider_context("output-display-provider"),
            InvocationStart::new("exec_command", json!({"cmd":"terminal-output"})),
        )
        .unwrap();
    let invocation_id = command.invocation_id.unwrap();
    for (sequence, text) in [
        "hello \u{1b}[31",
        "mred\u{1b}[0m\n",
        "\u{1b}]0;hidden",
        "\u{7}tail\n",
    ]
    .into_iter()
    .enumerate()
    {
        history.observe_output(SessionOutputChunk {
            internal_session_id: 77,
            session_handle: Arc::from("display-api-handle"),
            invocation: command.clone(),
            sequence: sequence as u64,
            text: text.to_string(),
        });
    }
    history.observe_output_complete(SessionOutputCompletion {
        internal_session_id: 77,
        session_handle: Arc::from("display-api-handle"),
        invocation: command.clone(),
    });
    complete(&history, &command, json!({"status":"exited","exit_code":0}));
    history.flush_for_test().unwrap();
    let app = router(history.clone());

    let detail = body_json(
        request(
            &app,
            &format!("/v1/invocations/{invocation_id}"),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert_eq!(detail["output"]["first_cursor"], 0);
    assert_eq!(detail["output"]["last_cursor"], 3);

    let metadata_response = request(
        &app,
        &format!("/v1/invocations/{invocation_id}/output-metadata"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(metadata_response.status(), StatusCode::OK);
    assert_eq!(
        metadata_response
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap(),
        "no-store"
    );
    let metadata_bytes = to_bytes(metadata_response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let metadata: Value = serde_json::from_slice(&metadata_bytes).unwrap();
    assert_eq!(metadata["invocation_id"], invocation_id);
    assert_eq!(metadata["output"]["first_cursor"], 0);
    assert_eq!(metadata["output"]["last_cursor"], 3);
    assert!(metadata.get("invocation").is_none());
    assert!(!String::from_utf8_lossy(&metadata_bytes).contains("terminal-output"));
    assert!(!String::from_utf8_lossy(&metadata_bytes).contains("hello"));

    let raw = body_json(
        request(
            &app,
            &format!("/v1/invocations/{invocation_id}/output?limit=2"),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert_eq!(raw["view"], "raw");
    assert!(raw.get("display_state").is_none());
    assert_eq!(raw["chunks"][0]["text"], "hello \u{1b}[31");

    let display = body_json(
        request(
            &app,
            &format!("/v1/invocations/{invocation_id}/output?view=display&limit=2"),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert_eq!(display["view"], "display");
    assert_eq!(display["display_state"], "available");
    assert_eq!(display["chunks"][0]["sequence"], 0);
    assert_eq!(display["chunks"][1]["sequence"], 1);
    assert_eq!(
        format!(
            "{}{}",
            display["chunks"][0]["text"].as_str().unwrap(),
            display["chunks"][1]["text"].as_str().unwrap()
        ),
        "hello red\n"
    );
    assert_eq!(display["next_cursor"], 2);

    let tail = body_json(
        request(
            &app,
            &format!("/v1/invocations/{invocation_id}/output?view=display&cursor=2&limit=8"),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert_eq!(tail["chunks"][0]["sequence"], 2);
    assert_eq!(tail["chunks"][0]["text"], "");
    assert_eq!(tail["chunks"][1]["sequence"], 3);
    assert_eq!(tail["chunks"][1]["text"], "tail\n");

    let unknown = request(
        &app,
        &format!("/v1/invocations/{invocation_id}/output?view=html"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);

    let connection = Connection::open(&database).unwrap();
    connection
        .execute(
            "DELETE FROM invocation_output_chunks WHERE invocation_id = ?1 AND sequence = 1",
            [invocation_id],
        )
        .unwrap();
    drop(connection);
    let degraded = body_json(
        request(
            &app,
            &format!("/v1/invocations/{invocation_id}/output?view=display&cursor=2&limit=8"),
            Some(TOKEN),
        )
        .await,
    )
    .await;
    assert_eq!(degraded["display_state"], "unavailable");
    assert!(degraded["chunks"].as_array().unwrap().is_empty());

    drop(app);
    history.shutdown_blocking().unwrap();
}

#[tokio::test]
async fn recovery_timeline_keeps_old_active_creator_then_drops_it_after_process_end() {
    let dir = tempdir().unwrap();
    let database = dir.path().join("history.sqlite3");
    let workdir = dir.path().join("repo");
    std::fs::create_dir_all(&workdir).unwrap();
    let history = open_history(&database, "runtime-active-timeline-recovery");
    let command = history
        .begin(
            provider_context("active-timeline-provider"),
            InvocationStart::new(
                "exec_command",
                json!({"cmd":"sleep 30","workdir":workdir.display().to_string()}),
            ),
        )
        .unwrap();
    let invocation_id = command.invocation_id.unwrap();
    complete(
        &history,
        &command,
        json!({
            "status":"running",
            "session_handle":"active-timeline-handle",
            "cwd":workdir.display().to_string()
        }),
    );
    history.flush_for_test().unwrap();
    let process = OwnedProcess {
        internal_session_id: 123,
        session_handle: Arc::from("active-timeline-handle"),
        identity: ProcessIdentity {
            pid: 99123,
            birth: ProcessBirthIdentity::LinuxProcStartTicks { ticks: 123 },
        },
        created_by: command,
    };
    history.process_started(&process).unwrap();
    let app = router(history.clone());
    let cutoff = i64::MAX / 4;

    let active_response = request(
        &app,
        &format!("/v1/timeline?limit=20&recovery_since_ms={cutoff}"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(active_response.status(), StatusCode::OK);
    let active = body_json(active_response).await;
    assert!(
        active["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| { record["primary_invocation_id"] == invocation_id })
    );

    history
        .process_ended(
            &process,
            &OwnedProcessEnd::exited(0, TerminationReason::Exit, workdir.display().to_string()),
        )
        .unwrap();
    history.flush_for_test().unwrap();
    let ended_response = request(
        &app,
        &format!("/v1/timeline?limit=20&recovery_since_ms={cutoff}"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(ended_response.status(), StatusCode::OK);
    let ended = body_json(ended_response).await;
    assert!(ended["records"].as_array().unwrap().is_empty());

    drop(app);
    history.shutdown_blocking().unwrap();
}
