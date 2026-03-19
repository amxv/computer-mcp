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
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::config::Config;
use crate::protocol::{ApplyPatchInput, ExecCommandInput, ToolOutput, WriteStdinInput};
use crate::service::ComputerService;

type McpHttpService = StreamableHttpService<ComputerMcpService, LocalSessionManager>;

#[derive(Clone)]
struct ComputerMcpService {
    computer_service: ComputerService,
    tool_router: ToolRouter<Self>,
}

impl ComputerMcpService {
    fn new(computer_service: ComputerService) -> Self {
        Self {
            computer_service,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl ComputerMcpService {
    #[tool(
        name = "exec_command",
        description = "Run a shell command",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn exec_command(
        &self,
        Parameters(input): Parameters<ExecCommandInput>,
    ) -> Result<McpJson<ToolOutput>, String> {
        self.computer_service
            .exec_command(input)
            .await
            .map(McpJson)
            .map_err(|e| e.to_string())
    }

    #[tool(
        name = "write_stdin",
        description = "Write to or poll a running session",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn write_stdin(
        &self,
        Parameters(input): Parameters<WriteStdinInput>,
    ) -> Result<McpJson<ToolOutput>, String> {
        self.computer_service
            .write_stdin(input)
            .await
            .map(McpJson)
            .map_err(|e| e.to_string())
    }

    #[tool(
        name = "apply_patch",
        description = "Apply a Codex-style patch to files",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn apply_patch(
        &self,
        Parameters(input): Parameters<ApplyPatchInput>,
    ) -> Result<String, String> {
        self.computer_service
            .apply_patch(input)
            .map_err(|e| e.to_string())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ComputerMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("computer-mcp remote execution tools")
    }
}

fn build_mcp_service(
    service: ComputerService,
    cancellation_token: CancellationToken,
) -> McpHttpService {
    StreamableHttpService::new(
        move || Ok(ComputerMcpService::new(service.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            cancellation_token,
            ..Default::default()
        },
    )
}

fn build_app(config: Arc<Config>, mcp_service: McpHttpService) -> Router {
    let protected_mcp_router = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(config, query_key_auth));

    Router::new()
        .route("/health", get(health))
        .merge(protected_mcp_router)
}

pub async fn run_server(config: Config) -> Result<()> {
    let bind = format!("{}:{}", config.bind_host, config.bind_port);
    let http_bind = config
        .http_bind_port
        .map(|port| format!("{}:{port}", config.bind_host));
    let cert_path = Path::new(&config.tls_cert_path);
    let key_path = Path::new(&config.tls_key_path);
    if !cert_path.exists() || !key_path.exists() {
        bail!(
            "TLS cert/key not found (cert: {}, key: {}). Run `computer-mcp start` or `computer-mcp tls setup` first.",
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
    let http_addr: Option<std::net::SocketAddr> = http_bind
        .as_deref()
        .map(|value| {
            value
                .parse()
                .with_context(|| format!("invalid HTTP bind address {value}"))
        })
        .transpose()?;

    let config = Arc::new(config);
    let computer_service = ComputerService::new(config.clone());

    let cancellation = CancellationToken::new();
    let mcp_service = build_mcp_service(computer_service, cancellation.child_token());
    let app = build_app(config, mcp_service);

    let handle = Handle::new();
    let shutdown_handle = handle.clone();
    let http_shutdown = cancellation.child_token();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        cancellation.cancel();
        shutdown_handle.graceful_shutdown(Some(Duration::from_secs(5)));
    });

    info!("computer-mcpd listening on https://{bind}");
    let tls_app = app.clone();
    let tls_server = async move {
        axum_server::bind_rustls(addr, rustls)
            .handle(handle)
            .serve(tls_app.into_make_service())
            .await
            .context("axum TLS server terminated unexpectedly")
    };

    if let Some(http_addr) = http_addr {
        info!("computer-mcpd also listening on http://{http_addr}");
        let listener = tokio::net::TcpListener::bind(http_addr)
            .await
            .with_context(|| format!("failed to bind HTTP listener on {http_addr}"))?;

        let http_server = async move {
            axum::serve(listener, app.into_make_service())
                .with_graceful_shutdown(async move {
                    http_shutdown.cancelled().await;
                })
                .await
                .context("axum HTTP server terminated unexpectedly")
        };

        let (_tls, _http) = tokio::try_join!(tls_server, http_server)?;
        Ok(())
    } else {
        tls_server.await
    }
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
    use super::ComputerMcpService;
    use crate::config::Config;
    use crate::service::ComputerService;
    use rmcp::model::ToolAnnotations;
    use std::sync::Arc;

    #[test]
    fn registers_apply_patch_tool() {
        let service = ComputerMcpService::new(ComputerService::new(Arc::new(Config::default())));
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

    #[test]
    fn tools_have_expected_annotations() {
        let service = ComputerMcpService::new(ComputerService::new(Arc::new(Config::default())));

        let by_name = |name: &str| {
            service
                .tool_router
                .list_all()
                .iter()
                .find(|tool| tool.name == name)
                .and_then(|tool| tool.annotations.clone())
                .unwrap_or_else(ToolAnnotations::default)
        };

        let exec = by_name("exec_command");
        assert_eq!(exec.read_only_hint, Some(true));
        assert_eq!(exec.destructive_hint, Some(false));
        assert_eq!(exec.open_world_hint, Some(false));

        let write = by_name("write_stdin");
        assert_eq!(write.read_only_hint, Some(true));
        assert_eq!(write.destructive_hint, Some(false));
        assert_eq!(write.open_world_hint, Some(false));

        let patch = by_name("apply_patch");
        assert_eq!(patch.read_only_hint, Some(true));
        assert_eq!(patch.destructive_hint, Some(false));
        assert_eq!(patch.open_world_hint, Some(false));
    }
}
