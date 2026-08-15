use std::sync::Arc;

use rusqlite::Connection;
use tempfile::tempdir;

use super::query::{HistoryQuery, LocalHistoryReader};
use super::schema::{HISTORY_SCHEMA_VERSION, SCHEMA_V1, SCHEMA_V2};
use super::store::HistoryStore;

#[test]
fn schema_v1_migrates_forward_to_current_file_evidence_schema() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "auto_vacuum", "INCREMENTAL")
        .unwrap();
    let mode: String = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .unwrap();
    assert!(mode.eq_ignore_ascii_case("wal"));
    connection.execute_batch(SCHEMA_V1).unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    drop(connection);

    assert!(
        LocalHistoryReader::query(&path, &HistoryQuery::recent(10))
            .unwrap()
            .is_empty(),
        "new binaries must keep stopped Phase-5 v1 history readable before the writer migrates it"
    );

    let store = HistoryStore::open(path.clone(), Arc::from("migration-runtime")).unwrap();
    drop(store);
    let connection = Connection::open(path).unwrap();
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, HISTORY_SCHEMA_VERSION);
    let table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'invocation_file_evidence'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1);
}

#[test]
fn schema_v2_migrates_forward_to_destination_before_evidence() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "auto_vacuum", "INCREMENTAL")
        .unwrap();
    let _: String = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .unwrap();
    connection.execute_batch(SCHEMA_V1).unwrap();
    connection.execute_batch(SCHEMA_V2).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);

    assert!(
        LocalHistoryReader::query(&path, &HistoryQuery::recent(10))
            .unwrap()
            .is_empty()
    );
    let store = HistoryStore::open(path.clone(), Arc::from("migration-runtime")).unwrap();
    drop(store);
    let connection = Connection::open(path).unwrap();
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, HISTORY_SCHEMA_VERSION);
    let destination_column_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('invocation_file_evidence') WHERE name = 'destination_before_state'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(destination_column_count, 1);
}
