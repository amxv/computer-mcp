use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::server::{LocalMcpServer, LocalMcpServerConfig, start_local_mcp_server};
use crate::service::ZodexService;
use crate::session::{
    OwnedProcess, OwnedProcessEnd, OwnedProcessObserver, ProcessIdentity, SessionRuntimePolicy,
};

use super::{
    LocalConfig, LocalHistoryRuntime, LocalHistoryRuntimeConfig, LocalObservabilityServer,
    LocalOwnedProcessRegistry, LocalPaths, start_local_observability_server,
};

pub struct LocalHostRuntimeOptions {
    pub paths: LocalPaths,
    pub start_directory: PathBuf,
    pub shell: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
    pub mcp_token: Arc<str>,
    pub shared_runtime_config: Arc<Config>,
    pub runtime_id: Option<Arc<str>>,
}

pub struct LocalHostRuntime {
    service: ZodexService,
    mcp_server: LocalMcpServer,
    observability_server: LocalObservabilityServer,
    history: Arc<LocalHistoryRuntime>,
}

struct LocalProcessObservers {
    registry: Arc<LocalOwnedProcessRegistry>,
    history: Arc<LocalHistoryRuntime>,
}

impl OwnedProcessObserver for LocalProcessObservers {
    fn process_started(&self, process: &OwnedProcess) -> Result<()> {
        self.history.process_started(process)?;
        self.registry.process_started(process)
    }

    fn process_group_members_updated(
        &self,
        process: &OwnedProcess,
        members: &[ProcessIdentity],
    ) -> Result<()> {
        self.registry
            .process_group_members_updated(process, members)
    }

    fn process_ended(&self, process: &OwnedProcess, end: &OwnedProcessEnd) -> Result<()> {
        let history_result = self.history.process_ended(process, end);
        let registry_result = self.registry.process_ended(process, end);
        history_result?;
        registry_result
    }
}

impl LocalHostRuntime {
    pub fn service(&self) -> &ZodexService {
        &self.service
    }

    pub fn mcp_url(&self) -> String {
        self.mcp_server.url()
    }

    pub fn mcp_addr(&self) -> std::net::SocketAddr {
        self.mcp_server.addr()
    }

    pub fn observability_url(&self) -> String {
        self.observability_server.base_url()
    }

    pub fn observability_addr(&self) -> std::net::SocketAddr {
        self.observability_server.addr()
    }

    pub fn runtime_id(&self) -> &str {
        self.history.runtime_id()
    }

    /// Close the side-effect admission boundary before external ingress is
    /// removed. The lifecycle owner calls this before terminating the tunnel.
    pub async fn close_admission(&self) {
        self.service.close_admission().await;
        self.mcp_server.request_shutdown();
    }

    pub async fn shutdown(self) -> Result<()> {
        self.close_admission().await;
        let Self {
            service,
            mcp_server,
            observability_server,
            history,
        } = self;
        let mut first_error = None;
        if let Err(error) = service.shutdown_sessions().await {
            first_error = Some(error.context("failed to stop Local command sessions"));
        }
        if let Err(error) = mcp_server.shutdown().await
            && first_error.is_none()
        {
            first_error = Some(error.context("failed to stop Local MCP listener"));
        }
        if let Err(error) = shutdown_history_bounded(history).await
            && first_error.is_none()
        {
            first_error = Some(error.context("failed to finalize Local history"));
        }
        if let Err(error) = observability_server.shutdown().await
            && first_error.is_none()
        {
            first_error = Some(error.context("failed to stop Local observability listener"));
        }
        first_error.map_or(Ok(()), Err)
    }
}

const HISTORY_SHUTDOWN_OUTER_TIMEOUT: Duration = Duration::from_secs(15);

async fn shutdown_history_bounded(history: Arc<LocalHistoryRuntime>) -> Result<()> {
    let (completed, result) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("zodex-local-history-shutdown".to_string())
        .spawn(move || {
            let _ = completed.send(history.shutdown_blocking());
        })
        .context("failed to start bounded Local history shutdown")?;
    match tokio::time::timeout(HISTORY_SHUTDOWN_OUTER_TIMEOUT, result).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(anyhow::anyhow!(
            "Local history shutdown worker ended without reporting a result"
        )),
        Err(_) => Err(anyhow::anyhow!(
            "Local history shutdown exceeded the bounded {}s deadline; runtime shutdown will continue",
            HISTORY_SHUTDOWN_OUTER_TIMEOUT.as_secs()
        )),
    }
}

/// Internal Local runtime constructor used by lifecycle integration and tests.
/// Public `zodex local start` remains owned by launchd lifecycle orchestration.
pub async fn start_local_host_runtime(
    options: LocalHostRuntimeOptions,
) -> Result<LocalHostRuntime> {
    let runtime_id = options
        .runtime_id
        .unwrap_or_else(|| Arc::<str>::from(format!("{:032x}", rand::random::<u128>())));
    let registry = Arc::new(LocalOwnedProcessRegistry::fresh(
        options.paths.owned_process_registry_file(),
        runtime_id.clone(),
    )?);
    let local_config = LocalConfig::load(&options.paths.config_file())?;
    let history = LocalHistoryRuntime::open(LocalHistoryRuntimeConfig::new(
        options.paths.history_database(),
        runtime_id,
        local_config.history.max_age.seconds(),
        local_config.history.max_size.bytes(),
    ))?;
    let process_observer = Arc::new(LocalProcessObservers {
        registry,
        history: history.clone(),
    });
    let policy = SessionRuntimePolicy::local(options.shell, options.environment)?
        .with_process_observer(process_observer)
        .with_output_observer(history.clone());
    let service = ZodexService::with_session_policy(options.shared_runtime_config, policy);
    let mcp_server = start_local_mcp_server(
        service.clone(),
        LocalMcpServerConfig::new(options.start_directory, options.mcp_token)
            .with_invocation_recorder(history.clone()),
    )
    .await?;
    let bearer = fs::read_to_string(options.paths.observability_bearer_file()).with_context(|| {
        format!(
            "failed to read Local observability bearer at {}",
            options.paths.observability_bearer_file().display()
        )
    });
    let observability_server = match bearer {
        Ok(bearer) => {
            match start_local_observability_server(history.clone(), bearer.trim()).await {
                Ok(server) => server,
                Err(error) => {
                    cleanup_failed_runtime_start(service.clone(), mcp_server, history.clone())
                        .await;
                    return Err(error.context("failed to start Local observability server"));
                }
            }
        }
        Err(error) => {
            cleanup_failed_runtime_start(service.clone(), mcp_server, history.clone()).await;
            return Err(error);
        }
    };
    Ok(LocalHostRuntime {
        service,
        mcp_server,
        observability_server,
        history,
    })
}

async fn cleanup_failed_runtime_start(
    service: ZodexService,
    mcp_server: LocalMcpServer,
    history: Arc<LocalHistoryRuntime>,
) {
    mcp_server.request_shutdown();
    let _ = service.shutdown_sessions().await;
    let _ = mcp_server.shutdown().await;
    let _ = tokio::task::spawn_blocking(move || history.shutdown_blocking()).await;
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{LocalHostRuntimeOptions, start_local_host_runtime};
    use crate::config::Config;
    use crate::local::{LocalPaths, ensure_observability_bearer};

    #[tokio::test]
    async fn host_runtime_keeps_mcp_and_observability_on_distinct_loopback_listeners() {
        let dir = tempdir().unwrap();
        let paths = LocalPaths::from_roots(
            dir.path().join("config"),
            dir.path().join("data"),
            dir.path().join("state"),
        )
        .unwrap();
        paths.ensure_persistent_dirs().unwrap();
        ensure_observability_bearer(&paths, false).unwrap();
        let workdir = dir.path().join("repo");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::create_dir_all(&home).unwrap();

        let runtime = start_local_host_runtime(LocalHostRuntimeOptions {
            paths,
            start_directory: workdir,
            shell: "/bin/sh".into(),
            environment: vec![
                ("HOME".into(), home.into_os_string()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ],
            mcp_token: "mcp-only-token".into(),
            shared_runtime_config: Config::default().into(),
            runtime_id: Some("runtime-fixture".into()),
        })
        .await
        .unwrap();

        assert!(runtime.mcp_addr().ip().is_loopback());
        assert!(runtime.observability_addr().ip().is_loopback());
        assert_ne!(runtime.mcp_addr(), runtime.observability_addr());
        assert_ne!(runtime.mcp_url(), runtime.observability_url());

        runtime.shutdown().await.unwrap();
    }
}
