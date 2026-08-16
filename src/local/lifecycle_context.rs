use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use super::LocalPaths;
use super::lifecycle::load_runtime_bootstrap;

pub fn paths_from_runtime_bootstrap(path: &Path) -> Result<LocalPaths> {
    let bootstrap = load_runtime_bootstrap(path)?;
    LocalPaths::from_roots(
        bootstrap.config_root,
        bootstrap.data_root,
        bootstrap.state_root,
    )
}

pub fn validate_runtime_start_directory(path: &Path) -> Result<PathBuf> {
    let canonical = canonicalize_start_directory(path)?;
    fs::read_dir(&canonical).map_err(|error| start_directory_error(&canonical, "read", error))?;
    Ok(canonical)
}

pub fn resolve_developer_shell(environment: &[(OsString, OsString)]) -> Result<PathBuf> {
    let shell = environment
        .iter()
        .find(|(key, value)| key == OsStr::new("SHELL") && !value.is_empty())
        .map(|(_, value)| PathBuf::from(value))
        .context(
            "captured Local developer environment is missing SHELL; start Zodex Local from the logged-in user's normal shell/session",
        )?;
    if !shell.is_absolute() {
        bail!(
            "captured SHELL must be an absolute path: {}",
            shell.display()
        );
    }
    let metadata = fs::metadata(&shell).with_context(|| {
        format!(
            "configured developer shell does not exist: {}",
            shell.display()
        )
    })?;
    if !metadata.is_file() {
        bail!(
            "configured developer shell is not a file: {}",
            shell.display()
        );
    }
    Ok(shell)
}

pub(super) fn canonicalize_start_directory(path: &Path) -> Result<PathBuf> {
    let metadata =
        fs::metadata(path).map_err(|error| start_directory_error(path, "inspect", error))?;
    if !metadata.is_dir() {
        bail!("Local start path is not a directory: {}", path.display());
    }
    fs::canonicalize(path).map_err(|error| start_directory_error(path, "resolve", error))
}

pub(super) fn start_directory_error(path: &Path, action: &str, error: io::Error) -> anyhow::Error {
    match error.kind() {
        io::ErrorKind::NotFound => {
            anyhow!("Local start directory does not exist: {}", path.display())
        }
        io::ErrorKind::PermissionDenied => anyhow!(
            "macOS denied permission to {action} Local start directory {}. Grant the responsible Zodex process access in System Settings > Privacy & Security > Files & Folders, or Full Disk Access when appropriate, then run `zodex local start` again: {error}",
            path.display()
        ),
        _ => anyhow!(
            "failed to {action} Local start directory {}: {error}",
            path.display()
        ),
    }
}
