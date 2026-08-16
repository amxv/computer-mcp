use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use super::LocalPaths;

pub(super) fn set_user_only_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to set 0700 permissions on {}", path.display()))?;
    }
    Ok(())
}

pub(super) fn write_private_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes =
        serde_json::to_vec_pretty(value).context("failed to encode Local runtime artifact")?;
    write_private_bytes(path, &bytes)
}

pub(super) fn write_private_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("Local runtime artifact has no parent")?;
    fs::create_dir_all(parent)?;
    set_user_only_directory(parent)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to write Local runtime artifact {}", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub(super) fn append_lifecycle_diagnostic(paths: &LocalPaths, message: &str) -> Result<()> {
    let path = paths.diagnostic_log_file();
    if fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
        > 2 * 1024 * 1024
    {
        let rotated = path.with_extension("log.1");
        let _ = fs::remove_file(&rotated);
        let _ = fs::rename(&path, &rotated);
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    writeln!(file, "{message}")?;
    Ok(())
}

pub(super) fn with_cleanup_error(primary: anyhow::Error, cleanup: Result<()>) -> anyhow::Error {
    match cleanup {
        Ok(()) => primary,
        Err(cleanup) => anyhow!(
            "{primary:#}; partial-start cleanup was also incomplete: {cleanup:#}. Run `zodex local status` before retrying so unresolved ownership is not discarded"
        ),
    }
}
