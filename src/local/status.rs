use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{
    LOCAL_OBSERVABILITY_API_VERSION, LocalConfig, LocalHistoryReader, LocalPaths,
    PRESENTATION_SCHEMA_VERSION,
};

pub const LOCAL_STATUS_SCHEMA_VERSION: u32 = 2;
pub const LOCAL_DISCOVERY_SCHEMA_VERSION: u32 = 1;
pub const LOCAL_RUNTIME_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalRuntimeState {
    pub schema_version: u32,
    pub runtime_id: String,
    pub lifecycle: LocalRuntimeLifecycle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalRuntimeLifecycle {
    Starting,
    Ready,
    Stopping,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalRuntimeDiscovery {
    pub schema_version: u32,
    pub runtime_id: String,
    pub pid: u32,
    pub start_directory: PathBuf,
    pub started_at: String,
    pub expires_at: Option<String>,
    pub observability: LocalObservabilityDiscovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalObservabilityDiscovery {
    pub api_version: u32,
    pub presentation_version: u32,
    pub base_url: String,
    pub bearer_token_path: PathBuf,
    pub history_available: bool,
    pub sse_available: bool,
}

impl LocalObservabilityDiscovery {
    pub fn active(base_url: impl Into<String>, bearer_token_path: impl Into<PathBuf>) -> Self {
        Self {
            api_version: LOCAL_OBSERVABILITY_API_VERSION,
            presentation_version: PRESENTATION_SCHEMA_VERSION,
            base_url: base_url.into(),
            bearer_token_path: bearer_token_path.into(),
            history_available: true,
            sse_available: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalStatusDocument {
    pub schema_version: u32,
    pub configured: bool,
    pub state: LocalStatusState,
    pub config_path: PathBuf,
    pub runtime_state_path: PathBuf,
    pub discovery_path: PathBuf,
    pub discovery: Option<LocalRuntimeDiscovery>,
    pub history: LocalHistoryStatus,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalStatusState {
    Unconfigured,
    Stopped,
    Running,
    Stale,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalHistoryStatus {
    pub database_path: PathBuf,
    pub database_exists: bool,
    pub physical_size_bytes: u64,
    pub store_state: String,
    pub store_reason: Option<String>,
    pub over_budget: bool,
    pub last_retention_error: Option<String>,
    pub max_age: String,
    pub max_age_seconds: u64,
    pub max_size: String,
    pub max_size_bytes: u64,
}

impl LocalStatusDocument {
    pub fn inspect(paths: &LocalPaths) -> Result<Self> {
        let config_path = paths.config_file();
        let config_exists = config_path.exists();
        let config = LocalConfig::load(&config_path)?;
        let runtime_state_path = paths.runtime_state_file();
        let discovery_path = paths.discovery_file();
        let runtime_state_present = runtime_state_path.exists();
        let discovery = load_discovery_if_present(paths)?;
        let history_store = LocalHistoryReader::status(&paths.history_database())?;

        let state = if runtime_state_present || discovery.is_some() {
            // Phase 2 deliberately does not claim a process is live merely because an
            // ephemeral marker survived. Native identity verification lands with lifecycle.
            LocalStatusState::Stale
        } else if !config_exists || !config.is_provider_configured() {
            LocalStatusState::Unconfigured
        } else {
            LocalStatusState::Stopped
        };

        Ok(Self {
            schema_version: LOCAL_STATUS_SCHEMA_VERSION,
            configured: config.is_provider_configured(),
            state,
            config_path,
            runtime_state_path,
            discovery_path,
            discovery,
            history: LocalHistoryStatus {
                database_path: paths.history_database(),
                database_exists: history_store.database_exists,
                physical_size_bytes: history_store.physical_size_bytes,
                store_state: history_store.health_state,
                store_reason: history_store.health_reason,
                over_budget: history_store.over_budget
                    || history_store.physical_size_bytes > config.history.max_size.bytes(),
                last_retention_error: history_store.last_retention_error,
                max_age: config.history.max_age.to_string(),
                max_age_seconds: config.history.max_age.seconds(),
                max_size: config.history.max_size.to_string(),
                max_size_bytes: config.history.max_size.bytes(),
            },
        })
    }
}

pub fn ensure_offline_mutation(paths: &LocalPaths, operation: &str) -> Result<()> {
    for marker in [paths.runtime_state_file(), paths.discovery_file()] {
        if marker.exists() {
            bail!(
                "cannot {operation} while Zodex Local runtime state is present at {}; run `zodex local stop` first",
                marker.display()
            );
        }
    }
    Ok(())
}

fn load_discovery_if_present(paths: &LocalPaths) -> Result<Option<LocalRuntimeDiscovery>> {
    let path = paths.discovery_file();
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read Local discovery at {}", path.display()))?;
    let discovery: LocalRuntimeDiscovery = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse Local discovery at {}", path.display()))?;
    if discovery.schema_version != LOCAL_DISCOVERY_SCHEMA_VERSION {
        bail!(
            "unsupported Local discovery schema version {} at {}; expected {}",
            discovery.schema_version,
            path.display(),
            LOCAL_DISCOVERY_SCHEMA_VERSION
        );
    }
    Ok(Some(discovery))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{
        LOCAL_DISCOVERY_SCHEMA_VERSION, LOCAL_RUNTIME_STATE_SCHEMA_VERSION,
        LocalObservabilityDiscovery, LocalRuntimeDiscovery, LocalRuntimeLifecycle,
        LocalRuntimeState, LocalStatusDocument, LocalStatusState, ensure_offline_mutation,
    };
    use crate::local::{LocalConfig, LocalPaths, ManagedTunnelClientRelease};

    fn test_paths() -> (tempfile::TempDir, LocalPaths) {
        let dir = tempdir().unwrap();
        let paths = LocalPaths::from_roots(
            dir.path().join("config"),
            dir.path().join("data"),
            dir.path().join("state"),
        )
        .unwrap();
        (dir, paths)
    }

    #[test]
    fn first_run_status_is_versioned_and_unconfigured() {
        let (_dir, paths) = test_paths();
        let status = LocalStatusDocument::inspect(&paths).unwrap();
        assert_eq!(status.schema_version, 2);
        assert!(!status.configured);
        assert_eq!(status.state, LocalStatusState::Unconfigured);
        assert_eq!(status.history.max_age_seconds, 60 * 24 * 60 * 60);
        assert_eq!(status.history.max_size_bytes, 500 * 1024 * 1024);
        assert!(status.discovery.is_none());
        assert!(!status.discovery_path.exists());
    }

    #[test]
    fn offline_mutation_guard_rejects_active_runtime_marker() {
        let (_dir, paths) = test_paths();
        std::fs::create_dir_all(paths.runtime_dir()).unwrap();
        let state = LocalRuntimeState {
            schema_version: LOCAL_RUNTIME_STATE_SCHEMA_VERSION,
            runtime_id: "runtime-test".to_string(),
            lifecycle: LocalRuntimeLifecycle::Ready,
        };
        std::fs::write(
            paths.runtime_state_file(),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();

        let error = ensure_offline_mutation(&paths, "change Local configuration").unwrap_err();
        assert!(error.to_string().contains("zodex local stop"));
    }

    #[test]
    fn discovery_schema_is_stable_and_status_treats_unverified_marker_as_stale() {
        let (_dir, paths) = test_paths();
        std::fs::create_dir_all(paths.runtime_dir()).unwrap();
        let discovery = LocalRuntimeDiscovery {
            schema_version: LOCAL_DISCOVERY_SCHEMA_VERSION,
            runtime_id: "runtime-test".to_string(),
            pid: 1234,
            start_directory: "/tmp/repo".into(),
            started_at: "2026-08-15T00:00:00Z".to_string(),
            expires_at: None,
            observability: LocalObservabilityDiscovery::active(
                "http://127.0.0.1:12345",
                paths.observability_bearer_file(),
            ),
        };
        std::fs::write(
            paths.discovery_file(),
            serde_json::to_vec(&discovery).unwrap(),
        )
        .unwrap();

        let status = LocalStatusDocument::inspect(&paths).unwrap();
        assert_eq!(status.state, LocalStatusState::Stale);
        assert_eq!(status.discovery, Some(discovery));
    }

    #[test]
    fn provider_fields_are_required_before_status_calls_configured() {
        let (_dir, paths) = test_paths();
        let mut config = LocalConfig::default();
        config.set("history.max-age", "30d").unwrap();
        config.save(&paths.config_file()).unwrap();
        assert!(!LocalStatusDocument::inspect(&paths).unwrap().configured);

        config
            .set("tunnel.id", "tunnel_0123456789abcdef0123456789abcdef")
            .unwrap();
        config.tunnel.client_path = Some(paths.managed_tunnel_client());
        config.save(&paths.config_file()).unwrap();
        assert!(!LocalStatusDocument::inspect(&paths).unwrap().configured);

        config.tunnel.release = Some(ManagedTunnelClientRelease {
            version: "v0.0.11".to_string(),
            asset_name: "tunnel-client-v0.0.11-darwin-arm64.zip".to_string(),
            archive_sha256: "a".repeat(64),
            binary_sha256: "b".repeat(64),
            cloudflared_sha256: "c".repeat(64),
            cloudflared_manifest_sha256: "d".repeat(64),
            source_url: "https://example.invalid/archive.zip".to_string(),
        });
        config.save(&paths.config_file()).unwrap();
        let status = LocalStatusDocument::inspect(&paths).unwrap();
        assert!(status.configured);
        assert_eq!(status.state, LocalStatusState::Stopped);
    }
}
