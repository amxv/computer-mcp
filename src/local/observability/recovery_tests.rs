use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{HeaderValue, Method, Request, header};
use serde_json::{Value, json};
use tempfile::tempdir;
use tower::ServiceExt as _;

use crate::invocation::{
    InvocationContext, InvocationEvidenceRecorder, InvocationOutcome, InvocationStart,
    ProviderCallMetadata,
};
use crate::local::{LocalHistoryRuntime, LocalHistoryRuntimeConfig};
use crate::session::{OwnedProcess, OwnedProcessObserver, ProcessBirthIdentity, ProcessIdentity};

use super::server::build_router;

const TOKEN: &str = "observability-recovery-token-0123456789abcdef";

#[tokio::test]
async fn recovery_list_includes_preexisting_active_process_creator_only_while_active() {
    let dir = tempdir().unwrap();
    let workdir = dir.path().join("repo");
    std::fs::create_dir_all(&workdir).unwrap();
    let history = LocalHistoryRuntime::open(LocalHistoryRuntimeConfig::new(
        dir.path().join("history.sqlite3"),
        "runtime-active-recovery",
        60 * 60,
        64 * 1024 * 1024,
    ))
    .unwrap();
    let command = history
        .begin(
            InvocationContext::default().with_provider(ProviderCallMetadata::new(
                "openai",
                "active-recovery-provider",
            )),
            InvocationStart::new(
                "exec_command",
                json!({
                    "cmd": "sleep 30",
                    "workdir": workdir.display().to_string(),
                    "yield_time_ms": 1000,
                }),
            ),
        )
        .unwrap();
    let invocation_id = command.invocation_id.unwrap();
    history
        .complete(
            &command,
            InvocationOutcome::Success(json!({
                "status": "running",
                "output": "",
                "summary": "running",
                "cwd": workdir.display().to_string(),
                "session_handle": "active-recovery-handle",
                "exit_code": null,
                "termination_reason": null,
            })),
        )
        .unwrap();
    history.flush_for_test().unwrap();

    let process = OwnedProcess {
        internal_session_id: 91,
        session_handle: Arc::from("active-recovery-handle"),
        identity: ProcessIdentity {
            pid: 9091,
            birth: ProcessBirthIdentity::LinuxProcStartTicks { ticks: 91 },
        },
        created_by: command,
    };
    history.process_started(&process).unwrap();
    let app = build_router(history.clone(), HeaderValue::from_static(TOKEN));
    let cutoff = i64::MAX / 4;

    let active = request_json(&app, cutoff).await;
    assert!(
        active["invocations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|invocation| invocation["id"] == invocation_id),
        "an active process creator that predates the recovery window must remain recoverable"
    );

    history.process_ended(&process).unwrap();
    let ended = request_json(&app, cutoff).await;
    assert!(
        ended["invocations"].as_array().unwrap().is_empty(),
        "ended process creators outside the recovery window must not become history preload"
    );

    drop(app);
    history.shutdown_blocking().unwrap();
}

async fn request_json(app: &axum::Router, cutoff: i64) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!(
                    "/v1/invocations?last=100&recovery_since_ms={cutoff}"
                ))
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
