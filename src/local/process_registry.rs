use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::session::{
    OwnedProcess, OwnedProcessObserver, ProcessControl, ProcessIdentity, ProcessSignal,
};

pub const LOCAL_PROCESS_REGISTRY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalProcessRegistryDocument {
    pub schema_version: u32,
    pub processes: Vec<LocalOwnedProcessRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalOwnedProcessRecord {
    pub internal_session_id: u64,
    pub session_handle: String,
    pub identity: ProcessIdentity,
    pub created_by_agent_id: Option<String>,
    pub invocation_correlation_id: Option<String>,
}

impl From<&OwnedProcess> for LocalOwnedProcessRecord {
    fn from(process: &OwnedProcess) -> Self {
        Self {
            internal_session_id: process.internal_session_id,
            session_handle: process.session_handle.to_string(),
            identity: process.identity.clone(),
            created_by_agent_id: process.created_by.agent_id.as_deref().map(str::to_owned),
            invocation_correlation_id: process
                .created_by
                .correlation_id
                .as_deref()
                .map(str::to_owned),
        }
    }
}

#[derive(Clone)]
pub struct LocalOwnedProcessRegistry {
    path: PathBuf,
    records: Arc<Mutex<Vec<LocalOwnedProcessRecord>>>,
}

impl LocalOwnedProcessRegistry {
    pub fn fresh(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if path.exists() {
            let existing = load_document(&path)?;
            if !existing.processes.is_empty() {
                bail!(
                    "Local owned-process registry at {} still contains {} process record(s); stale recovery must resolve it before a fresh runtime starts",
                    path.display(),
                    existing.processes.len()
                );
            }
            fs::remove_file(&path).with_context(|| {
                format!("failed to remove empty stale registry {}", path.display())
            })?;
        }
        Ok(Self {
            path,
            records: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub fn snapshot(&self) -> Vec<LocalOwnedProcessRecord> {
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn update(&self, mutate: impl FnOnce(&mut Vec<LocalOwnedProcessRecord>)) -> Result<()> {
        let mut guard = self
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut next = guard.clone();
        mutate(&mut next);
        persist_records(&self.path, &next)?;
        *guard = next;
        Ok(())
    }
}

impl OwnedProcessObserver for LocalOwnedProcessRegistry {
    fn process_started(&self, process: &OwnedProcess) -> Result<()> {
        let record = LocalOwnedProcessRecord::from(process);
        self.update(|records| {
            records.retain(|existing| existing.internal_session_id != record.internal_session_id);
            records.push(record);
        })
    }

    fn process_ended(&self, process: &OwnedProcess) -> Result<()> {
        self.update(|records| {
            records.retain(|record| record.internal_session_id != process.internal_session_id);
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaleProcessCleanupReport {
    pub signaled: usize,
    pub already_gone: usize,
    pub identity_mismatch: usize,
}

pub fn signal_matching_stale_processes(
    registry_path: &Path,
    control: &dyn ProcessControl,
    signal: ProcessSignal,
) -> Result<StaleProcessCleanupReport> {
    if !registry_path.exists() {
        return Ok(StaleProcessCleanupReport::default());
    }
    let document = load_document(registry_path)?;
    let mut report = StaleProcessCleanupReport::default();
    for process in document.processes {
        match control.identity(process.identity.pid)? {
            None => report.already_gone += 1,
            Some(current) if current != process.identity => report.identity_mismatch += 1,
            Some(_) => {
                control.signal_group(process.identity.pid, signal)?;
                report.signaled += 1;
            }
        }
    }
    Ok(report)
}

fn load_document(path: &Path) -> Result<LocalProcessRegistryDocument> {
    let raw = fs::read(path)
        .with_context(|| format!("failed to read Local process registry {}", path.display()))?;
    let document: LocalProcessRegistryDocument = serde_json::from_slice(&raw)
        .with_context(|| format!("failed to parse Local process registry {}", path.display()))?;
    if document.schema_version != LOCAL_PROCESS_REGISTRY_SCHEMA_VERSION {
        bail!(
            "unsupported Local process registry schema version {} at {}; expected {}",
            document.schema_version,
            path.display(),
            LOCAL_PROCESS_REGISTRY_SCHEMA_VERSION
        );
    }
    Ok(document)
}

fn persist_records(path: &Path, records: &[LocalOwnedProcessRecord]) -> Result<()> {
    if records.is_empty() {
        if path.exists() {
            fs::remove_file(path).with_context(|| {
                format!(
                    "failed to remove empty Local process registry {}",
                    path.display()
                )
            })?;
        }
        return Ok(());
    }

    let parent = path
        .parent()
        .context("Local process registry path has no parent")?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create Local runtime directory {}",
            parent.display()
        )
    })?;
    set_user_only_directory_permissions(parent)?;

    let document = LocalProcessRegistryDocument {
        schema_version: LOCAL_PROCESS_REGISTRY_SCHEMA_VERSION,
        processes: records.to_vec(),
    };
    let encoded =
        serde_json::to_vec(&document).context("failed to encode Local process registry")?;
    let temp = parent.join(format!(
        ".owned-processes.{}.{:016x}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));

    let result = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp)
            .with_context(|| format!("failed to create temporary registry {}", temp.display()))?;
        file.write_all(&encoded)
            .with_context(|| format!("failed to write temporary registry {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temporary registry {}", temp.display()))?;
        fs::rename(&temp, path).with_context(|| {
            format!(
                "failed to publish Local process registry {} -> {}",
                temp.display(),
                path.display()
            )
        })?;
        set_user_only_file_permissions(path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(unix)]
fn set_user_only_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to set 0700 permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_user_only_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_user_only_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set 0600 permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_user_only_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use tempfile::tempdir;

    use crate::invocation::InvocationContext;
    use crate::session::{
        OwnedProcess, OwnedProcessObserver, ProcessBirthIdentity, ProcessControl, ProcessIdentity,
        ProcessInspector, ProcessSignal,
    };

    use super::{LocalOwnedProcessRegistry, signal_matching_stale_processes};

    fn identity(pid: i32, ticks: u64) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            birth: ProcessBirthIdentity::LinuxProcStartTicks { ticks },
        }
    }

    fn owned_process(id: u64, pid: i32, ticks: u64) -> OwnedProcess {
        OwnedProcess {
            internal_session_id: id,
            session_handle: format!("handle{id}").into(),
            identity: identity(pid, ticks),
            created_by: InvocationContext {
                invocation_id: None,
                correlation_id: Some(format!("invoke-{id}").into()),
                provider: None,
                agent_id: Some("k7m2".into()),
            },
        }
    }

    #[test]
    fn registry_atomically_tracks_start_and_end_without_leaving_empty_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runtime/owned-processes.json");
        let registry = LocalOwnedProcessRegistry::fresh(&path).unwrap();
        let first = owned_process(1, 1001, 11);
        let second = owned_process(2, 1002, 22);

        registry.process_started(&first).unwrap();
        registry.process_started(&second).unwrap();
        assert_eq!(registry.snapshot().len(), 2);
        assert!(path.exists());
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(raw["schema_version"], 1);
        assert_eq!(raw["processes"].as_array().unwrap().len(), 2);

        registry.process_ended(&first).unwrap();
        assert_eq!(registry.snapshot().len(), 1);
        registry.process_ended(&second).unwrap();
        assert!(registry.snapshot().is_empty());
        assert!(!path.exists(), "empty ephemeral registry should be removed");
    }

    struct FakeControl {
        identities: HashMap<i32, ProcessIdentity>,
        signaled: Mutex<Vec<(i32, ProcessSignal)>>,
    }

    impl ProcessInspector for FakeControl {
        fn identity(&self, pid: i32) -> anyhow::Result<Option<ProcessIdentity>> {
            Ok(self.identities.get(&pid).cloned())
        }

        fn live_cwd(&self, _pid: i32) -> Option<String> {
            None
        }
    }

    impl ProcessControl for FakeControl {
        fn signal_group(&self, pid: i32, signal: ProcessSignal) -> anyhow::Result<()> {
            self.signaled.lock().unwrap().push((pid, signal));
            Ok(())
        }
    }

    #[test]
    fn stale_cleanup_signals_only_matching_birth_identity() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runtime/owned-processes.json");
        let registry = LocalOwnedProcessRegistry::fresh(&path).unwrap();
        let matching = owned_process(1, 2001, 101);
        let reused_pid = owned_process(2, 2002, 202);
        let gone = owned_process(3, 2003, 303);
        registry.process_started(&matching).unwrap();
        registry.process_started(&reused_pid).unwrap();
        registry.process_started(&gone).unwrap();

        let control = FakeControl {
            identities: HashMap::from([(2001, identity(2001, 101)), (2002, identity(2002, 999))]),
            signaled: Mutex::new(Vec::new()),
        };
        let report =
            signal_matching_stale_processes(&path, &control, ProcessSignal::Terminate).unwrap();

        assert_eq!(report.signaled, 1);
        assert_eq!(report.identity_mismatch, 1);
        assert_eq!(report.already_gone, 1);
        assert_eq!(
            *control.signaled.lock().unwrap(),
            vec![(2001, ProcessSignal::Terminate)]
        );
    }

    #[test]
    fn fresh_registry_refuses_to_overwrite_unresolved_ownership() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runtime/owned-processes.json");
        let registry = LocalOwnedProcessRegistry::fresh(&path).unwrap();
        registry.process_started(&owned_process(1, 42, 1)).unwrap();
        let error = LocalOwnedProcessRegistry::fresh(&path)
            .err()
            .expect("unresolved registry should block fresh runtime");
        assert!(error.to_string().contains("stale recovery"));
    }
}
