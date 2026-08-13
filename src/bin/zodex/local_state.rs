#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LocalSetupState {
    Provisioning,
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalTargetRecord {
    version: u32,
    machine_id: String,
    setup_state: LocalSetupState,
    #[serde(default)]
    image_reference: Option<String>,
    #[serde(default)]
    requested_cpus: Option<u32>,
    #[serde(default)]
    requested_memory: Option<String>,
    #[serde(default)]
    network: Option<LocalNetworkExpectation>,
    #[serde(default)]
    setup_sources: Option<LocalSetupSources>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalNetworkExpectation {
    policy_version: u32,
    namespace: String,
    root_interface: String,
    agent_interface: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalSetupSources {
    repo: String,
    reader_app_id: u64,
    reader_pem_path: String,
    reader_installation_id: u64,
    publisher_app_id: u64,
    publisher_pem_path: String,
    publisher_installation_id: u64,
    default_base: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalAccessLease {
    version: u32,
    generation: String,
    created_at_epoch_seconds: u64,
    expires_at_epoch_seconds: u64,
    active: bool,
}

fn local_target_state_path_from_home(home: &Path) -> PathBuf {
    home.join(LOCAL_TARGET_STATE_RELATIVE_PATH)
}

fn local_access_lease_path_from_home(home: &Path) -> PathBuf {
    home.join(LOCAL_ACCESS_LEASE_RELATIVE_PATH)
}

fn local_state_paths() -> Result<(PathBuf, PathBuf)> {
    let home = env::var("HOME").context("HOME must be set to inspect Zodex Local state")?;
    let home = Path::new(&home);
    Ok((
        local_target_state_path_from_home(home),
        local_access_lease_path_from_home(home),
    ))
}

fn load_local_state_file<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read(path).with_context(|| format!("failed to read {label} at {}", path.display()))?;
    serde_json::from_slice(&raw)
        .with_context(|| format!("failed to parse {label} at {}", path.display()))
        .map(Some)
}

fn load_local_target_record(path: &Path) -> Result<Option<LocalTargetRecord>> {
    let record: Option<LocalTargetRecord> = load_local_state_file(path, "Local target state")?;
    if let Some(record) = &record {
        if record.version != 1 {
            bail!("unsupported Local target state version {}", record.version);
        }
        if record.machine_id != LOCAL_MACHINE_NAME {
            bail!(
                "Local target state names machine `{}`; expected `{LOCAL_MACHINE_NAME}`",
                record.machine_id
            );
        }
        if record.setup_state == LocalSetupState::Ready && record.setup_sources.is_none() {
            bail!("ready Local target state is missing setup source references");
        }
        if record.setup_state == LocalSetupState::Ready && record.network.is_none() {
            bail!("ready Local target state is missing network policy identity");
        }
    }
    Ok(record)
}

fn load_local_access_lease(path: &Path) -> Result<Option<LocalAccessLease>> {
    let lease: Option<LocalAccessLease> = load_local_state_file(path, "Local access lease")?;
    if let Some(lease) = &lease {
        if lease.version != 1 {
            bail!("unsupported Local access lease version {}", lease.version);
        }
        if lease.generation.trim().is_empty() {
            bail!("Local access lease generation must not be empty");
        }
        if lease.expires_at_epoch_seconds <= lease.created_at_epoch_seconds {
            bail!("Local access lease expiration must be after creation");
        }
    }
    Ok(lease)
}

#[allow(dead_code)]
fn save_local_state_file<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("{label} path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    #[cfg(unix)]
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to secure {}", parent.display()))?;

    let raw = serde_json::to_vec_pretty(value).with_context(|| format!("failed to encode {label}"))?;
    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary {label} in {}", parent.display()))?;
    #[cfg(unix)]
    temp.as_file().set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to secure temporary {label}"))?;
    temp.write_all(&raw).with_context(|| format!("failed to write temporary {label}"))?;
    temp.as_file_mut().sync_all().with_context(|| format!("failed to sync temporary {label}"))?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to atomically replace {label} at {}", path.display()))?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to secure {}", path.display()))?;
    Ok(())
}

#[allow(dead_code)]
fn save_local_target_record(path: &Path, record: &LocalTargetRecord) -> Result<()> {
    save_local_state_file(path, record, "Local target state")
}

#[allow(dead_code)]
fn save_local_access_lease(path: &Path, lease: &LocalAccessLease) -> Result<()> {
    save_local_state_file(path, lease, "Local access lease")
}
