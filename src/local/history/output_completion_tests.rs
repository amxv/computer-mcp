use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

use crate::invocation::{
    InvocationContext, InvocationEvidenceRecorder, InvocationOutcome, InvocationStart,
    ProviderCallMetadata,
};
use crate::session::{SessionOutputChunk, SessionOutputCompletion, SessionOutputObserver};

use super::query::{HistoryQuery, LocalHistoryReader};
use super::worker::{LocalHistoryRuntime, LocalHistoryRuntimeConfig};

#[test]
fn oversized_raw_output_is_bounded_to_one_invocation_without_degrading_history() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history/history.sqlite3");
    let runtime = LocalHistoryRuntime::open(LocalHistoryRuntimeConfig::new(
        path.clone(),
        "runtime",
        365 * 86_400,
        1024 * 1024 * 1024,
    ))
    .unwrap();
    let context = runtime
        .begin(
            InvocationContext::default()
                .with_correlation_id("oversized-output")
                .with_provider(ProviderCallMetadata::new("openai/session", "provider")),
            InvocationStart::new(
                "exec_command",
                json!({"cmd":"huge-output","workdir":dir.path()}),
            ),
        )
        .unwrap();
    let invocation_id = context.invocation_id.unwrap();

    let started = Instant::now();
    for sequence in 0..320 {
        runtime.observe_output(SessionOutputChunk {
            internal_session_id: 1,
            session_handle: Arc::from("oversizedoutput000000"),
            invocation: context.clone(),
            sequence,
            text: "x".repeat(8192),
        });
    }
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "oversized audit capture backpressured its producer: {:?}",
        started.elapsed()
    );
    runtime.observe_output_complete(SessionOutputCompletion {
        internal_session_id: 1,
        session_handle: Arc::from("oversizedoutput000000"),
        invocation: context.clone(),
    });
    runtime.flush_for_test().unwrap();
    runtime
        .complete(
            &context,
            InvocationOutcome::Success(json!({"status":"exited","output":"model-result"})),
        )
        .unwrap();
    runtime.flush_for_test().unwrap();

    let record = LocalHistoryReader::query(
        &path,
        &HistoryQuery {
            last: 1,
            invocation_id: Some(invocation_id),
            include_raw: true,
            ..HistoryQuery::default()
        },
    )
    .unwrap()
    .pop()
    .unwrap();
    assert_eq!(record.evidence_state, "complete");
    assert_eq!(record.capture_state, "incomplete");
    assert!(
        record
            .capture_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("2097152 bytes"))
    );
    assert!(record.full_output.unwrap_or_default().len() <= 2 * 1024 * 1024);
    assert!(!runtime.history_degraded());
    assert_eq!(
        LocalHistoryReader::status(&path).unwrap().health_state,
        "healthy"
    );
    runtime.shutdown_blocking().unwrap();
}

#[test]
fn terminal_output_completion_never_waits_for_history_capacity() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history/history.sqlite3");
    let runtime = LocalHistoryRuntime::open(
        LocalHistoryRuntimeConfig::new(path.clone(), "runtime", 365 * 86_400, 1024 * 1024 * 1024)
            .with_output_queue_capacity(1),
    )
    .unwrap();
    let context = runtime
        .begin(
            InvocationContext::default()
                .with_correlation_id("terminal-capacity")
                .with_provider(ProviderCallMetadata::new("openai/session", "provider")),
            InvocationStart::new(
                "exec_command",
                json!({
                    "cmd": "printf terminal-capacity",
                    "workdir": dir.path(),
                    "yield_time_ms": 1000,
                }),
            ),
        )
        .unwrap();
    let invocation_id = context.invocation_id.unwrap();

    let locker = Connection::open(&path).unwrap();
    locker.execute_batch("BEGIN IMMEDIATE").unwrap();
    runtime.observe_output(SessionOutputChunk {
        internal_session_id: 1,
        session_handle: Arc::from("terminalcapacity00000"),
        invocation: context.clone(),
        sequence: 0,
        text: "first".to_string(),
    });
    // Let the writer dequeue the first event and block on the SQLite writer
    // lock, then occupy the sole bounded queue slot with a second chunk.
    std::thread::sleep(Duration::from_millis(50));
    runtime.observe_output(SessionOutputChunk {
        internal_session_id: 1,
        session_handle: Arc::from("terminalcapacity00000"),
        invocation: context.clone(),
        sequence: 1,
        text: "second".to_string(),
    });
    // The sole queue slot is now occupied while SQLite remains locked. This
    // third chunk must be retained by the bounded transient overflow path,
    // not dropped and not allowed to backpressure the producer.
    runtime.observe_output(SessionOutputChunk {
        internal_session_id: 1,
        session_handle: Arc::from("terminalcapacity00000"),
        invocation: context.clone(),
        sequence: 2,
        text: "third".to_string(),
    });
    assert!(
        !runtime.history_degraded(),
        "fixture unexpectedly overflowed an ordinary output chunk"
    );

    let started = Instant::now();
    runtime.observe_output_complete(SessionOutputCompletion {
        internal_session_id: 1,
        session_handle: Arc::from("terminalcapacity00000"),
        invocation: context.clone(),
    });
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "history completion backpressured the PTY reader: {:?}",
        started.elapsed()
    );

    locker.execute_batch("ROLLBACK").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    runtime.flush_for_test().unwrap();
    runtime
        .complete(
            &context,
            InvocationOutcome::Success(json!({"status":"exited","output":"firstsecondthird"})),
        )
        .unwrap();
    runtime.flush_for_test().unwrap();
    runtime.shutdown_blocking().unwrap();

    let record = LocalHistoryReader::query(
        &path,
        &HistoryQuery {
            last: 1,
            invocation_id: Some(invocation_id),
            include_raw: true,
            ..HistoryQuery::default()
        },
    )
    .unwrap()
    .pop()
    .unwrap();
    assert_eq!(record.evidence_state, "complete");
    assert_eq!(record.capture_state, "incomplete");
    let captured = record.full_output.as_deref().unwrap_or_default();
    assert!(captured.starts_with("first"));
    assert!(!captured.contains("third"));
    assert!(
        record
            .capture_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("queue"))
    );
    assert_eq!(
        LocalHistoryReader::status(&path).unwrap().health_state,
        "healthy"
    );
}
