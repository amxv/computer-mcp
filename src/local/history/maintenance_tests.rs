use std::sync::Arc;

use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

use crate::invocation::{
    InvocationContext, InvocationOutcome, InvocationStart, ProviderCallMetadata,
};

use super::query::{HistoryQuery, LocalHistoryReader};
use super::store::HistoryStore;
use super::worker::{LocalHistoryRuntime, LocalHistoryRuntimeConfig};

#[test]
fn startup_retention_finishes_before_runtime_is_returned() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let store = HistoryStore::open(path.clone(), Arc::from("runtime-old")).unwrap();
    let context = store
        .begin(
            InvocationContext::default()
                .with_correlation_id("expired-before-start")
                .with_provider(ProviderCallMetadata::new("openai/session", "provider-old")),
            InvocationStart::new("exec_command", json!({"cmd":"true","workdir":dir.path()})),
        )
        .unwrap();
    store
        .complete(
            &context,
            InvocationOutcome::Success(json!({"status":"exited","exit_code":0})),
        )
        .unwrap();
    drop(store);

    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE invocations SET started_at_ms = 1, completed_at_ms = 2",
            [],
        )
        .unwrap();
    drop(connection);

    let runtime = LocalHistoryRuntime::open(LocalHistoryRuntimeConfig::new(
        path.clone(),
        "runtime-new",
        1,
        u64::MAX,
    ))
    .unwrap();
    assert!(
        LocalHistoryReader::query(&path, &HistoryQuery::recent(10))
            .unwrap()
            .is_empty(),
        "startup retention must complete before the runtime can admit new work"
    );
    runtime.shutdown_blocking().unwrap();
}
