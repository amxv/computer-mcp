use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::config::Config;
use crate::server::{LocalMcpServer, LocalMcpServerConfig, start_local_mcp_server};
use crate::service::ZodexService;
use crate::session::SessionRuntimePolicy;

use super::{LocalOwnedProcessRegistry, LocalPaths};

pub struct LocalHostRuntimeOptions {
    pub paths: LocalPaths,
    pub start_directory: PathBuf,
    pub shell: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
    pub mcp_token: Arc<str>,
    pub shared_runtime_config: Arc<Config>,
}

pub struct LocalHostRuntime {
    service: ZodexService,
    mcp_server: LocalMcpServer,
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

    pub async fn shutdown(self) -> Result<()> {
        let Self {
            service,
            mcp_server,
        } = self;
        // Stop accepting new MCP traffic before closing session admission, but
        // do not await graceful HTTP drain first: an in-flight long-yield tool
        // call must be preempted by whole-runtime session shutdown rather than
        // delaying it.
        mcp_server.request_shutdown();
        let session_shutdown = service.shutdown_sessions().await;
        let mcp_shutdown = mcp_server.shutdown().await;
        session_shutdown?;
        mcp_shutdown?;
        Ok(())
    }
}

/// Internal Local runtime constructor used by lifecycle integration and tests.
/// Public `zodex local start` remains owned by the later launchd lifecycle phase.
pub async fn start_local_host_runtime(
    options: LocalHostRuntimeOptions,
) -> Result<LocalHostRuntime> {
    let registry = LocalOwnedProcessRegistry::fresh(options.paths.owned_process_registry_file())?;
    let policy = SessionRuntimePolicy::local(options.shell, options.environment)?
        .with_process_observer(Arc::new(registry));
    let service = ZodexService::with_session_policy(options.shared_runtime_config, policy);
    let mcp_server = start_local_mcp_server(
        service.clone(),
        LocalMcpServerConfig::new(options.start_directory, options.mcp_token),
    )
    .await?;
    Ok(LocalHostRuntime {
        service,
        mcp_server,
    })
}
