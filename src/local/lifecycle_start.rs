use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use super::lifecycle::{
    cleanup_partial_start, cleanup_stale_runtime, healthy_existing_discovery, prepare_local_launch,
    wait_for_runtime_ready,
};
use super::lifecycle_artifacts::with_cleanup_error;
use super::lifecycle_lock::LocalLifecycleLock;
use super::{
    LaunchdController, LocalPaths, LocalRuntimeDiscovery, LocalStatusDocument, load_runtime_state,
};

const START_READY_TIMEOUT: Duration = Duration::from_secs(60);
const START_ATTEMPTS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalStartOutcome {
    pub discovery: LocalRuntimeDiscovery,
    pub already_running: bool,
    pub current_runtime_agent_count: usize,
    pub active_process_count: usize,
}

pub async fn start_via_launchd(
    paths: &LocalPaths,
    executable: &Path,
    requested_start_directory: &Path,
    ttl_seconds: Option<u64>,
    environment: &[(OsString, OsString)],
    launchd: &dyn LaunchdController,
) -> Result<LocalStartOutcome> {
    start_via_launchd_with_timeout(
        paths,
        executable,
        requested_start_directory,
        ttl_seconds,
        environment,
        launchd,
        START_READY_TIMEOUT,
    )
    .await
}

pub(super) async fn start_via_launchd_with_timeout(
    paths: &LocalPaths,
    executable: &Path,
    requested_start_directory: &Path,
    ttl_seconds: Option<u64>,
    environment: &[(OsString, OsString)],
    launchd: &dyn LaunchdController,
    ready_timeout: Duration,
) -> Result<LocalStartOutcome> {
    let _lifecycle_lock = LocalLifecycleLock::acquire(paths)?;
    if let Some(discovery) = healthy_existing_discovery(paths)? {
        return outcome(paths, discovery, true);
    }
    cleanup_stale_runtime(paths, launchd)?;

    let mut first_timeout = None;
    for attempt in 0..START_ATTEMPTS {
        let prepared = prepare_local_launch(
            paths,
            executable,
            requested_start_directory,
            ttl_seconds,
            environment,
        )?;
        if let Err(error) = launchd.bootstrap(&prepared.plist_path) {
            return Err(with_cleanup_error(
                error.context("failed to bootstrap Zodex Local launchd runtime"),
                cleanup_partial_start(paths, launchd),
            ));
        }
        match wait_for_runtime_ready(paths, &prepared.runtime_id, ready_timeout).await {
            Ok(discovery) => return outcome(paths, discovery, false),
            Err(error) => {
                let retry =
                    attempt == 0 && runtime_never_published_process(paths, &prepared.runtime_id)?;
                if let Err(cleanup_error) = cleanup_partial_start(paths, launchd) {
                    return Err(with_cleanup_error(error, Err(cleanup_error)));
                }
                if retry {
                    first_timeout = Some(error);
                    continue;
                }
                return match first_timeout {
                    Some(first) => Err(error.context(format!(
                        "Local launchd retry also failed after the first job never published a process: {first:#}"
                    ))),
                    None => Err(error),
                };
            }
        }
    }
    unreachable!("the bounded Local launch attempt loop always returns")
}

fn runtime_never_published_process(paths: &LocalPaths, runtime_id: &str) -> Result<bool> {
    Ok(load_runtime_state(paths)?
        .is_some_and(|state| state.runtime_id == runtime_id && state.process.is_none()))
}

fn outcome(
    paths: &LocalPaths,
    discovery: LocalRuntimeDiscovery,
    already_running: bool,
) -> Result<LocalStartOutcome> {
    let status = LocalStatusDocument::inspect(paths)?;
    Ok(LocalStartOutcome {
        discovery,
        already_running,
        current_runtime_agent_count: status.current_runtime_agent_count,
        active_process_count: status.active_process_count,
    })
}
