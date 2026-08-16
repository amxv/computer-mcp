use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension};

pub const HISTORY_SCHEMA_VERSION: u32 = 3;

pub(super) const SCHEMA_V1: &str = r#"
CREATE TABLE agents (
    id TEXT PRIMARY KEY CHECK(length(id) = 4),
    provider_kind TEXT NOT NULL,
    provider_session_key TEXT NOT NULL,
    first_seen_at_ms INTEGER NOT NULL,
    last_seen_at_ms INTEGER NOT NULL,
    last_seen_runtime_id TEXT NOT NULL,
    UNIQUE(provider_kind, provider_session_key)
);

CREATE TABLE invocations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    correlation_id TEXT NOT NULL UNIQUE,
    agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    provider_kind TEXT,
    provider_session_key TEXT,
    tool_name TEXT NOT NULL,
    args_json TEXT NOT NULL,
    declared_workdir_exact TEXT,
    declared_workdir_normalized TEXT,
    is_new_workdir INTEGER NOT NULL DEFAULT 0 CHECK(is_new_workdir IN (0, 1)),
    started_at_ms INTEGER NOT NULL,
    completed_at_ms INTEGER,
    duration_ms INTEGER,
    outcome_kind TEXT CHECK(outcome_kind IN ('success', 'error')),
    result_json TEXT,
    error_text TEXT,
    evidence_state TEXT NOT NULL CHECK(evidence_state IN ('pending', 'complete', 'incomplete')),
    evidence_reason TEXT,
    capture_state TEXT NOT NULL CHECK(capture_state IN ('not_applicable', 'pending', 'complete', 'incomplete')),
    capture_reason TEXT,
    target_session_handle TEXT,
    target_created_by_agent_id TEXT,
    cross_agent INTEGER CHECK(cross_agent IS NULL OR cross_agent IN (0, 1)),
    result_status TEXT,
    result_cwd TEXT,
    result_session_handle TEXT,
    result_exit_code INTEGER,
    result_termination_reason TEXT
);

CREATE INDEX invocations_started_idx ON invocations(started_at_ms DESC, id DESC);
CREATE INDEX invocations_agent_idx ON invocations(agent_id, started_at_ms DESC, id DESC);
CREATE INDEX invocations_workdir_idx ON invocations(declared_workdir_normalized, started_at_ms DESC, id DESC);
CREATE INDEX invocations_provider_idx ON invocations(provider_kind, provider_session_key, id DESC);

CREATE TABLE invocation_output_chunks (
    invocation_id INTEGER NOT NULL REFERENCES invocations(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    observed_at_ms INTEGER NOT NULL,
    text TEXT NOT NULL,
    PRIMARY KEY(invocation_id, sequence)
) WITHOUT ROWID;

CREATE TABLE agent_workdirs (
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    normalized_workdir TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    first_seen_at_ms INTEGER NOT NULL,
    last_seen_at_ms INTEGER NOT NULL,
    first_invocation_id INTEGER NOT NULL,
    last_invocation_id INTEGER NOT NULL,
    retained_invocation_count INTEGER NOT NULL,
    PRIMARY KEY(agent_id, normalized_workdir),
    UNIQUE(agent_id, ordinal)
);

CREATE TABLE history_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    health_state TEXT NOT NULL,
    health_reason TEXT,
    over_budget INTEGER NOT NULL DEFAULT 0 CHECK(over_budget IN (0, 1)),
    last_retention_error TEXT,
    updated_at_ms INTEGER NOT NULL
);

INSERT INTO history_state(singleton, health_state, updated_at_ms)
VALUES (1, 'healthy', 0);
"#;

pub(super) const SCHEMA_V2: &str = r#"
CREATE TABLE invocation_file_evidence (
    invocation_id INTEGER NOT NULL REFERENCES invocations(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    source_kind TEXT NOT NULL CHECK(source_kind IN ('apply_patch', 'shell_write')),
    operation_hint TEXT NOT NULL CHECK(operation_hint IN ('create', 'update', 'delete', 'move', 'overwrite', 'append')),
    path_before TEXT NOT NULL,
    path_after TEXT NOT NULL,
    before_state TEXT NOT NULL CHECK(before_state IN ('missing', 'text', 'unavailable')),
    before_text TEXT,
    before_reason TEXT,
    after_state TEXT NOT NULL CHECK(after_state IN ('pending', 'missing', 'text', 'unavailable')),
    after_text TEXT,
    after_reason TEXT,
    source_after_state TEXT CHECK(source_after_state IS NULL OR source_after_state IN ('missing', 'text', 'unavailable')),
    source_after_text TEXT,
    source_after_reason TEXT,
    PRIMARY KEY(invocation_id, ordinal)
) WITHOUT ROWID;

CREATE INDEX invocation_file_evidence_after_path_idx
ON invocation_file_evidence(path_after, invocation_id);
"#;

const SCHEMA_V3: &str = r#"
ALTER TABLE invocation_file_evidence ADD COLUMN destination_before_state TEXT
    CHECK(destination_before_state IS NULL OR destination_before_state IN ('missing', 'text', 'unavailable'));
ALTER TABLE invocation_file_evidence ADD COLUMN destination_before_text TEXT;
ALTER TABLE invocation_file_evidence ADD COLUMN destination_before_reason TEXT;
"#;

pub(super) fn configure_writer(connection: &Connection) -> Result<()> {
    connection
        .busy_timeout(std::time::Duration::from_millis(500))
        .context("failed to configure Local history SQLite busy timeout")?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .context("failed to enable Local history foreign keys")?;
    Ok(())
}

pub(super) fn initialize_or_migrate(connection: &mut Connection) -> Result<()> {
    configure_writer(connection)?;
    let mut version = user_version(connection)?;
    if version > HISTORY_SCHEMA_VERSION {
        bail!(
            "Local history schema version {version} is newer than this Zodex build supports ({HISTORY_SCHEMA_VERSION})"
        );
    }

    if version == 0 {
        let existing_table: Option<String> = connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .context("failed to inspect unversioned Local history database")?;
        if let Some(table) = existing_table {
            bail!(
                "refusing to initialize unversioned non-empty Local history database; found table `{table}`"
            );
        }

        // auto_vacuum must be selected before the v1 tables are created. WAL
        // is enabled on the same empty database so physical-retention behavior
        // is part of the schema-creation contract rather than a later retrofit.
        connection
            .pragma_update(None, "auto_vacuum", "INCREMENTAL")
            .context("failed to enable incremental auto-vacuum for Local history")?;
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .context("failed to enable WAL mode for Local history")?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            bail!("Local history SQLite refused WAL mode (reported `{journal_mode}`)");
        }

        let transaction = connection
            .transaction()
            .context("failed to start Local history schema-v1 transaction")?;
        transaction
            .execute_batch(SCHEMA_V1)
            .context("failed to create Local history schema v1")?;
        transaction
            .pragma_update(None, "user_version", 1)
            .context("failed to publish Local history schema version")?;
        transaction
            .commit()
            .context("failed to commit Local history schema v1")?;
        version = 1;
    }

    if version >= 1 {
        let auto_vacuum: i64 = connection
            .pragma_query_value(None, "auto_vacuum", |row| row.get(0))
            .context("failed to inspect Local history auto-vacuum mode")?;
        if auto_vacuum != 2 {
            bail!(
                "Local history schema v1 requires incremental auto-vacuum; database reports mode {auto_vacuum}"
            );
        }
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .context("failed to restore WAL mode for Local history")?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            bail!("Local history SQLite refused WAL mode (reported `{journal_mode}`)");
        }
    }

    if version == 1 {
        let transaction = connection
            .transaction()
            .context("failed to start Local history schema-v2 transaction")?;
        transaction
            .execute_batch(SCHEMA_V2)
            .context("failed to create Local history schema v2")?;
        transaction
            .pragma_update(None, "user_version", 2)
            .context("failed to publish Local history schema version 2")?;
        transaction
            .commit()
            .context("failed to commit Local history schema v2")?;
        version = 2;
    }

    if version == 2 {
        let transaction = connection
            .transaction()
            .context("failed to start Local history schema-v3 transaction")?;
        transaction
            .execute_batch(SCHEMA_V3)
            .context("failed to create Local history schema v3")?;
        transaction
            .pragma_update(None, "user_version", 3)
            .context("failed to publish Local history schema version 3")?;
        transaction
            .commit()
            .context("failed to commit Local history schema v3")?;
    }

    let version = user_version(connection)?;
    if version != HISTORY_SCHEMA_VERSION {
        bail!(
            "Local history schema migration ended at version {version}; expected {HISTORY_SCHEMA_VERSION}"
        );
    }
    Ok(())
}

pub(super) fn verify_readable_schema(connection: &Connection) -> Result<()> {
    readable_schema_version(connection).map(|_| ())
}

pub(super) fn readable_schema_version(connection: &Connection) -> Result<u32> {
    let version = user_version(connection)?;
    if version == 0 || version > HISTORY_SCHEMA_VERSION {
        bail!(
            "unsupported Local history schema version {version}; supported range is 1..={HISTORY_SCHEMA_VERSION}"
        );
    }
    Ok(version)
}

fn user_version(connection: &Connection) -> Result<u32> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .context("failed to inspect Local history schema version")
}
