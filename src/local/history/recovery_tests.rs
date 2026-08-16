use std::collections::HashSet;
use std::sync::Arc;

use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

use crate::invocation::{
    InvocationContext, InvocationOutcome, InvocationStart, ProviderCallMetadata,
};

use super::query::{HistoryQuery, LocalHistoryReader};
use super::store::HistoryStore;

fn history_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join("history/history.sqlite3")
}

fn provider_context(session: &str, correlation: &str) -> InvocationContext {
    InvocationContext::default()
        .with_correlation_id(correlation)
        .with_provider(ProviderCallMetadata::new("openai/session", session))
}

fn exec_start(workdir: &std::path::Path, command: &str) -> InvocationStart {
    InvocationStart::new(
        "exec_command",
        json!({
            "cmd": command,
            "workdir": workdir,
            "yield_time_ms": 1000,
        }),
    )
}

fn complete_ok(store: &HistoryStore, context: &InvocationContext, marker: &str) {
    store
        .complete(
            context,
            InvocationOutcome::Success(json!({
                "status": "exited",
                "output": marker,
                "summary": marker,
                "cwd": "/tmp",
                "session_handle": null,
                "exit_code": 0,
                "termination_reason": null,
            })),
        )
        .unwrap();
}

#[test]
fn recovery_window_keeps_preexisting_running_and_changed_invocations_only() {
    let dir = tempdir().unwrap();
    let path = history_path(dir.path());
    let store = HistoryStore::open(path.clone(), Arc::from("runtime-recovery")).unwrap();

    let old_completed = store
        .begin(
            provider_context("provider-old", "old-completed"),
            exec_start(dir.path(), "old-completed"),
        )
        .unwrap();
    complete_ok(&store, &old_completed, "old-completed");
    let old_running = store
        .begin(
            provider_context("provider-running", "old-running"),
            exec_start(dir.path(), "old-running"),
        )
        .unwrap();
    let changed_completed = store
        .begin(
            provider_context("provider-changed", "changed-completed"),
            exec_start(dir.path(), "changed-completed"),
        )
        .unwrap();
    complete_ok(&store, &changed_completed, "changed-completed");
    let newly_started = store
        .begin(
            provider_context("provider-new", "newly-started"),
            exec_start(dir.path(), "newly-started"),
        )
        .unwrap();
    complete_ok(&store, &newly_started, "newly-started");
    drop(store);

    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE invocations SET started_at_ms = 100, completed_at_ms = 110 WHERE id = ?1",
            [old_completed.invocation_id.unwrap()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE invocations SET started_at_ms = 100, completed_at_ms = NULL WHERE id = ?1",
            [old_running.invocation_id.unwrap()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE invocations SET started_at_ms = 100, completed_at_ms = 210 WHERE id = ?1",
            [changed_completed.invocation_id.unwrap()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE invocations SET started_at_ms = 220, completed_at_ms = 230 WHERE id = ?1",
            [newly_started.invocation_id.unwrap()],
        )
        .unwrap();
    drop(connection);

    let records = LocalHistoryReader::query(
        &path,
        &HistoryQuery {
            last: 20,
            active_or_changed_since_ms: Some(200),
            ..HistoryQuery::default()
        },
    )
    .unwrap();
    let correlations = records
        .iter()
        .map(|record| record.correlation_id.as_str())
        .collect::<HashSet<_>>();
    assert_eq!(
        correlations,
        HashSet::from(["old-running", "changed-completed", "newly-started"])
    );
    assert!(!correlations.contains("old-completed"));
}
