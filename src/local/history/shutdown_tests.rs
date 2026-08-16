use serde_json::json;
use tempfile::tempdir;

use crate::invocation::{
    InvocationContext, InvocationEvidenceRecorder, InvocationOutcome, InvocationStart,
};

use super::{HistoryQuery, LocalHistoryReader, LocalHistoryRuntime, LocalHistoryRuntimeConfig};

#[test]
fn shutdown_marks_unfinished_pty_capture_incomplete_instead_of_leaving_pending() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let runtime = LocalHistoryRuntime::open(LocalHistoryRuntimeConfig::new(
        path.clone(),
        "shutdown-capture-test",
        365 * 86_400,
        1024 * 1024 * 1024,
    ))
    .unwrap();
    let context = runtime
        .begin(
            InvocationContext::default(),
            InvocationStart::new(
                "exec_command",
                json!({"cmd":"sleep 30", "workdir":dir.path()}),
            ),
        )
        .unwrap();
    runtime
        .complete(
            &context,
            InvocationOutcome::Success(json!({"status":"running"})),
        )
        .unwrap();

    runtime.shutdown_blocking().unwrap();

    let record = LocalHistoryReader::query(&path, &HistoryQuery::recent(1))
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(record.evidence_state, "complete");
    assert_eq!(record.capture_state, "incomplete");
    assert!(
        record
            .capture_reason
            .as_deref()
            .unwrap_or_default()
            .contains("shutdown ended before PTY capture")
    );
}
