use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{Json as McpJson, ServerHandler, tool, tool_handler, tool_router};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::apply_patch;
use crate::config::Config;
use crate::protocol::{ApplyPatchInput, ExecCommandInput, ToolOutput, WriteStdinInput};
use crate::session::SessionManager;

#[derive(Clone)]
struct SharedState {
    config: Arc<Config>,
    sessions: Arc<Mutex<SessionManager>>,
}

#[derive(Clone)]
struct ComputerMcpService {
    state: SharedState,
    tool_router: ToolRouter<Self>,
}

impl ComputerMcpService {
    fn new(state: SharedState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl ComputerMcpService {
    #[tool(name = "exec_command", description = "Run a shell command")]
    async fn exec_command(
        &self,
        Parameters(input): Parameters<ExecCommandInput>,
    ) -> Result<McpJson<ToolOutput>, String> {
        let mut sessions = self.state.sessions.lock().await;
        sessions
            .exec_command(input, &self.state.config)
            .await
            .map(McpJson)
            .map_err(|e| e.to_string())
    }

    #[tool(
        name = "write_stdin",
        description = "Write to or poll a running session"
    )]
    async fn write_stdin(
        &self,
        Parameters(input): Parameters<WriteStdinInput>,
    ) -> Result<McpJson<ToolOutput>, String> {
        let mut sessions = self.state.sessions.lock().await;
        sessions
            .write_stdin(input, &self.state.config)
            .await
            .map(McpJson)
            .map_err(|e| e.to_string())
    }

    #[tool(
        name = "apply_patch",
        description = "Apply a Codex-style patch to files"
    )]
    async fn apply_patch(
        &self,
        Parameters(input): Parameters<ApplyPatchInput>,
    ) -> Result<String, String> {
        apply_patch::apply_patch(&input.patch).map_err(|e| e.to_string())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ComputerMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("computer-mcp remote execution tools")
    }
}

pub async fn run_server(config: Config) -> Result<()> {
    let bind = format!("{}:{}", config.bind_host, config.bind_port);
    let cert_path = Path::new(&config.tls_cert_path);
    let key_path = Path::new(&config.tls_key_path);
    if !cert_path.exists() || !key_path.exists() {
        bail!(
            "TLS cert/key not found (cert: {}, key: {}). Run `computer-mcp tls setup` first.",
            config.tls_cert_path,
            config.tls_key_path
        );
    }

    let rustls = RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .with_context(|| {
            format!(
                "failed to load TLS cert/key from {} and {}",
                config.tls_cert_path, config.tls_key_path
            )
        })?;
    let addr: std::net::SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid bind address {bind}"))?;

    let shared_state = SharedState {
        sessions: Arc::new(Mutex::new(SessionManager::new(
            config.max_sessions,
            config.max_output_chars,
        ))),
        config: Arc::new(config),
    };

    let cancellation = CancellationToken::new();
    let mcp_service: StreamableHttpService<ComputerMcpService, LocalSessionManager> =
        StreamableHttpService::new(
            {
                let state = shared_state.clone();
                move || Ok(ComputerMcpService::new(state.clone()))
            },
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig {
                cancellation_token: cancellation.child_token(),
                ..Default::default()
            },
        );

    let protected_mcp_router =
        Router::new()
            .nest_service("/mcp", mcp_service)
            .layer(middleware::from_fn_with_state(
                shared_state.config.clone(),
                query_key_auth,
            ));

    let app = Router::new()
        .route("/health", get(health))
        .merge(protected_mcp_router);

    let handle = Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        cancellation.cancel();
        shutdown_handle.graceful_shutdown(Some(Duration::from_secs(5)));
    });

    info!("computer-mcpd listening on https://{bind}");
    axum_server::bind_rustls(addr, rustls)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .context("axum TLS server terminated unexpectedly")
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn query_key_auth(
    State(config): State<Arc<Config>>,
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let supplied_key = key_from_query(request.uri().query());

    if supplied_key.as_deref() == Some(config.api_key.as_str()) {
        return Ok(next.run(request).await);
    }

    Err(StatusCode::UNAUTHORIZED)
}

fn key_from_query(query: Option<&str>) -> Option<String> {
    let query = query?;

    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key == "key" {
            return Some(value.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{ComputerMcpService, SharedState};
    use crate::config::Config;
    use crate::session::SessionManager;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn registers_apply_patch_tool() {
        let state = SharedState {
            config: Arc::new(Config::default()),
            sessions: Arc::new(Mutex::new(SessionManager::new(64, 200_000))),
        };

        let service = ComputerMcpService::new(state);
        let names: Vec<String> = service
            .tool_router
            .list_all()
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();

        assert!(names.iter().any(|name| name == "exec_command"));
        assert!(names.iter().any(|name| name == "write_stdin"));
        assert!(names.iter().any(|name| name == "apply_patch"));
    }
}
