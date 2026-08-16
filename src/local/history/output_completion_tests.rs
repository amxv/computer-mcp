use std::sync::{Arc, mpsc};
use std::time::Duration;

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
fn terminal_output_completion_waits_for_capacity_without_losing_capture() {
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
        runtime.accepting_new_invocations(),
        "fixture unexpectedly overflowed an ordinary output chunk"
    );

    let runtime_for_completion = runtime.clone();
    let context_for_completion = context.clone();
    let (completed, completion_received) = mpsc::channel();
    let completion_thread = std::thread::spawn(move || {
        runtime_for_completion.observe_output_complete(SessionOutputCompletion {
            internal_session_id: 1,
            session_handle: Arc::from("terminalcapacity00000"),
            invocation: context_for_completion,
        });
        completed.send(()).unwrap();
    });
    assert!(
        completion_received
            .recv_timeout(Duration::from_millis(50))
            .is_err(),
        "terminal completion should wait instead of dropping when the queue is full"
    );

    locker.execute_batch("ROLLBACK").unwrap();
    completion_received
        .recv_timeout(Duration::from_secs(2))
        .expect("terminal completion should enqueue after writer capacity returns");
    completion_thread.join().unwrap();
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
    assert_eq!(record.capture_state, "complete");
    assert_eq!(record.full_output.as_deref(), Some("firstsecondthird"));
    assert_eq!(
        LocalHistoryReader::status(&path).unwrap().health_state,
        "healthy"
    );
}
