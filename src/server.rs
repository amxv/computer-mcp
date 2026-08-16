use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use axum::extract::{Request, State};
use axum::http::{StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{RequestMetaObject, ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{Json as McpJson, ServerHandler, tool, tool_handler, tool_router};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::{ServiceExt, service_fn};
use tracing::info;

use crate::config::Config;
use crate::http_api;
use crate::protocol::{ApplyPatchInput, ExecCommandInput, ToolOutput, WriteStdinInput};
use crate::service::{ServiceRequest, ZodexService};
use crate::session::SessionOrigin;

type McpHttpService = StreamableHttpService<ZodexMcpService, LocalSessionManager>;
const DEFAULT_MCP_INSTRUCTIONS: &str = "zodex remote execution tools";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProviderMetadata {
    openai_session: Option<String>,
}

fn extract_provider_metadata(meta: &RequestMetaObject) -> ProviderMetadata {
    ProviderMetadata {
        openai_session: meta
            .get("openai/session")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

type ProviderMetadataObserver = Arc<dyn Fn(&ProviderMetadata) + Send + Sync>;

#[derive(Clone)]
struct McpServerPolicy {
    legacy_session_mode: bool,
    json_response: bool,
    stateless_protocol_metadata_required: bool,
    disable_allowed_hosts: bool,
    instructions: Arc<str>,
    provider_metadata_observer: Option<ProviderMetadataObserver>,
}

impl Default for McpServerPolicy {
    fn default() -> Self {
        Self {
            legacy_session_mode: true,
            json_response: false,
            stateless_protocol_metadata_required: false,
            disable_allowed_hosts: true,
            instructions: Arc::from(DEFAULT_MCP_INSTRUCTIONS),
            provider_metadata_observer: None,
        }
    }
}

#[derive(Clone)]
struct ZodexMcpService {
    zodex_service: ZodexService,
    tool_router: ToolRouter<Self>,
    instructions: Arc<str>,
    provider_metadata_observer: Option<ProviderMetadataObserver>,
}

impl ZodexMcpService {
    #[cfg(test)]
    fn new(zodex_service: ZodexService) -> Self {
        Self::with_options(zodex_service, Arc::from(DEFAULT_MCP_INSTRUCTIONS), None)
    }

    fn with_options(
        zodex_service: ZodexService,
        instructions: Arc<str>,
        provider_metadata_observer: Option<ProviderMetadataObserver>,
    ) -> Self {
        Self {
            zodex_service,
            tool_router: Self::tool_router(),
            instructions,
            provider_metadata_observer,
        }
    }

    fn observe_provider_metadata(&self, meta: &RequestMetaObject) {
        if let Some(observer) = &self.provider_metadata_observer {
            observer(&extract_provider_metadata(meta));
        }
    }

    async fn execute_tool_output(
        &self,
        request: ServiceRequest,
    ) -> Result<McpJson<ToolOutput>, String> {
        self.zodex_service
            .execute(request)
            .await
            .and_then(|response| response.into_tool_output())
            .map(McpJson)
            .map_err(|e| e.to_string())
    }

    async fn execute_apply_patch(&self, input: ApplyPatchInput) -> Result<String, String> {
        self.zodex_service
            .execute(ServiceRequest::ApplyPatch { input })
            .await
            .and_then(|response| response.into_apply_patch_output())
            .map(|output| output.output)
            .map_err(|e| e.to_string())
    }
}

#[tool_router]
impl ZodexMcpService {
    #[tool(
        name = "exec_command",
        description = "Run a shell command in a required absolute existing workdir",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn exec_command(
        &self,
        Parameters(input): Parameters<ExecCommandInput>,
        request_meta: RequestMetaObject,
    ) -> Result<McpJson<ToolOutput>, String> {
        self.observe_provider_metadata(&request_meta);
        self.execute_tool_output(ServiceRequest::ExecCommand {
            input,
            origin: SessionOrigin::mcp(None),
        })
        .await
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
        request_meta: RequestMetaObject,
    ) -> Result<McpJson<ToolOutput>, String> {
        self.observe_provider_metadata(&request_meta);
        self.execute_tool_output(ServiceRequest::WriteStdin { input })
            .await
    }

    #[tool(
        name = "apply_patch",
        description = "Apply a Codex-style patch using a required absolute existing workdir",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn apply_patch(
        &self,
        Parameters(input): Parameters<ApplyPatchInput>,
        request_meta: RequestMetaObject,
    ) -> Result<String, String> {
        self.observe_provider_metadata(&request_meta);
        self.execute_apply_patch(input).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ZodexMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(self.instructions.to_string())
    }
}

fn build_mcp_service(
    service: ZodexService,
    cancellation_token: CancellationToken,
) -> McpHttpService {
    build_mcp_service_with_policy(service, cancellation_token, McpServerPolicy::default())
}

fn build_mcp_service_with_policy(
    service: ZodexService,
    cancellation_token: CancellationToken,
    policy: McpServerPolicy,
) -> McpHttpService {
    let instructions = policy.instructions.clone();
    let provider_metadata_observer = policy.provider_metadata_observer.clone();
    let mut config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(policy.legacy_session_mode)
        .with_json_response(policy.json_response)
        .with_stateless_protocol_metadata_required(policy.stateless_protocol_metadata_required)
        .with_cancellation_token(cancellation_token);
    if policy.disable_allowed_hosts {
        config = config.disable_allowed_hosts();
    }
    StreamableHttpService::new(
        move || {
            Ok(ZodexMcpService::with_options(
                service.clone(),
                instructions.clone(),
                provider_metadata_observer.clone(),
            ))
        },
        LocalSessionManager::default().into(),
        config,
    )
}

fn build_app(
    config: Arc<Config>,
    mcp_service: McpHttpService,
    zodex_service: ZodexService,
) -> Router {
    let mcp_auth_config = config.clone();
    let http_auth_config = config;
    let mcp_root_service = |mcp_service: McpHttpService| {
        service_fn(move |mut request: Request| {
            let mcp_service = mcp_service.clone();
            async move {
                let uri = rewrite_mcp_transport_root_uri(request.uri())
                    .expect("mcp root service only handles /mcp and /mcp/");
                *request.uri_mut() = uri;

                let response = mcp_service
                    .oneshot(request)
                    .await
                    .unwrap_or_else(|never| match never {});
                Ok::<_, Infallible>(response)
            }
        })
    };
    let protected_mcp_router = Router::new()
        .route_service("/mcp", mcp_root_service(mcp_service.clone()))
        .route_service("/mcp/", mcp_root_service(mcp_service))
        .layer(middleware::from_fn_with_state(
            mcp_auth_config,
            query_key_auth,
        ));
    let http_api_router = http_api::build_http_api_router(http_auth_config, zodex_service);

    Router::new()
        .route("/health", get(health))
        .merge(protected_mcp_router)
        .merge(http_api_router)
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
            "TLS cert/key not found (cert: {}, key: {}). Run `zodex start` or `zodex tls setup` first.",
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
    let zodex_service = ZodexService::new(config.clone());

    let cancellation = CancellationToken::new();
    let mcp_service = build_mcp_service(zodex_service.clone(), cancellation.child_token());
    let app = build_app(config, mcp_service, zodex_service);

    let handle = Handle::new();
    let shutdown_handle = handle.clone();
    let http_shutdown = cancellation.child_token();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        cancellation.cancel();
        shutdown_handle.graceful_shutdown(Some(Duration::from_secs(5)));
    });

    info!("zodexd listening on https://{bind}");
    let tls_app = app.clone();
    let tls_server = async move {
        axum_server::bind_rustls(addr, rustls)
            .handle(handle)
            .serve(tls_app.into_make_service())
            .await
            .context("axum TLS server terminated unexpectedly")
    };

    if let Some(http_addr) = http_addr {
        info!("zodexd also listening on http://{http_addr}");
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

fn rewrite_mcp_transport_root_uri(uri: &Uri) -> Option<Uri> {
    if uri.path() != "/mcp" && uri.path() != "/mcp/" {
        return None;
    }

    let mut parts = uri.clone().into_parts();
    let path_and_query = match uri.query() {
        Some(query) => format!("/?{query}"),
        None => "/".to_string(),
    };
    parts.path_and_query = Some(path_and_query.parse().ok()?);
    Uri::from_parts(parts).ok()
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
mod tests;
