use std::convert::Infallible;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::{StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{RequestMetaObject, ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{Json as McpJson, ServerHandler, tool, tool_handler, tool_router};
use serde::Serialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::{ServiceExt, service_fn};
use tracing::{error, info};

use crate::config::Config;
use crate::invocation::{
    InvocationContext, InvocationEvidenceRecorder, InvocationOutcome, InvocationStart,
    ProviderCallMetadata,
};
use crate::protocol::{ApplyPatchInput, ExecCommandInput, ToolOutput, WriteStdinInput};
use crate::service::{ServiceRequest, ZodexService};
use crate::session::SessionOrigin;

type McpHttpService = StreamableHttpService<ZodexMcpService, LocalSessionManager>;
const DEFAULT_MCP_INSTRUCTIONS: &str = "zodex remote execution tools";

mod local;

pub use local::{
    LOCAL_MCP_TOKEN_HEADER, LocalMcpServer, LocalMcpServerConfig, start_local_mcp_server,
};

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

fn invocation_context(meta: &RequestMetaObject) -> InvocationContext {
    let provider = extract_provider_metadata(meta);
    let mut context = InvocationContext::default()
        .with_correlation_id(format!("{:032x}", rand::random::<u128>()));
    if let Some(session_key) = provider.openai_session {
        context = context.with_provider(ProviderCallMetadata::new("openai/session", session_key));
    }
    context
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
    invocation_recorder: Option<Arc<dyn InvocationEvidenceRecorder>>,
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
            invocation_recorder: None,
        }
    }
}

#[derive(Clone)]
struct ZodexMcpService {
    zodex_service: ZodexService,
    tool_router: ToolRouter<Self>,
    instructions: Arc<str>,
    provider_metadata_observer: Option<ProviderMetadataObserver>,
    invocation_recorder: Option<Arc<dyn InvocationEvidenceRecorder>>,
}

impl ZodexMcpService {
    #[cfg(test)]
    fn new(zodex_service: ZodexService) -> Self {
        Self::with_options(
            zodex_service,
            Arc::from(DEFAULT_MCP_INSTRUCTIONS),
            None,
            None,
        )
    }

    fn with_options(
        zodex_service: ZodexService,
        instructions: Arc<str>,
        provider_metadata_observer: Option<ProviderMetadataObserver>,
        invocation_recorder: Option<Arc<dyn InvocationEvidenceRecorder>>,
    ) -> Self {
        Self {
            zodex_service,
            tool_router: Self::tool_router(),
            instructions,
            provider_metadata_observer,
            invocation_recorder,
        }
    }

    fn observe_provider_metadata(&self, meta: &RequestMetaObject) {
        if let Some(observer) = &self.provider_metadata_observer {
            observer(&extract_provider_metadata(meta));
        }
    }

    fn begin_invocation<T: Serialize>(
        &self,
        tool_name: &'static str,
        input: &T,
        request_meta: &RequestMetaObject,
        target_created_by_agent_id: Option<Arc<str>>,
    ) -> Result<InvocationContext, String> {
        self.observe_provider_metadata(request_meta);
        let context = invocation_context(request_meta);
        let Some(recorder) = &self.invocation_recorder else {
            return Ok(context);
        };
        let arguments = serde_json::to_value(input).map_err(|error| {
            format!("failed to serialize {tool_name} invocation input: {error}")
        })?;
        recorder
            .begin(
                context,
                InvocationStart::new(tool_name, arguments)
                    .with_target_created_by_agent_id(target_created_by_agent_id),
            )
            .map_err(|error| format!("Local invocation evidence unavailable: {error}"))
    }

    fn complete_invocation<T: Serialize>(
        &self,
        context: &InvocationContext,
        result: &Result<T, String>,
    ) {
        let Some(recorder) = &self.invocation_recorder else {
            return;
        };
        let outcome = match result {
            Ok(value) => match serde_json::to_value(value) {
                Ok(value) => InvocationOutcome::Success(value),
                Err(error) => {
                    error!(
                        event = "local_invocation_result_serialization_failed",
                        invocation_id = ?context.invocation_id,
                        error = %error,
                    );
                    InvocationOutcome::Error(format!(
                        "internal evidence serialization failure after tool completion: {error}"
                    ))
                }
            },
            Err(error) => InvocationOutcome::Error(error.clone()),
        };
        if let Err(error) = recorder.complete(context, outcome) {
            error!(
                event = "local_invocation_completion_persistence_failed",
                invocation_id = ?context.invocation_id,
                error = %error,
            );
        }
    }

    async fn execute_tool_output(
        &self,
        request: ServiceRequest,
        invocation: InvocationContext,
    ) -> Result<ToolOutput, String> {
        self.zodex_service
            .execute_with_context(request, invocation)
            .await
            .and_then(|response| response.into_tool_output())
            .map_err(|e| e.to_string())
    }

    async fn execute_apply_patch(
        &self,
        input: ApplyPatchInput,
        invocation: InvocationContext,
    ) -> Result<String, String> {
        self.zodex_service
            .execute_with_context(ServiceRequest::ApplyPatch { input }, invocation)
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
        let invocation = self.begin_invocation("exec_command", &input, &request_meta, None)?;
        let result = self
            .execute_tool_output(
                ServiceRequest::ExecCommand {
                    input,
                    origin: SessionOrigin::mcp(None),
                },
                invocation.clone(),
            )
            .await;
        self.complete_invocation(&invocation, &result);
        result.map(McpJson)
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
        let target_created_by_agent_id = self
            .zodex_service
            .session_creator_agent_id(&input.session_handle)
            .await;
        let invocation = self.begin_invocation(
            "write_stdin",
            &input,
            &request_meta,
            target_created_by_agent_id,
        )?;
        let result = self
            .execute_tool_output(ServiceRequest::WriteStdin { input }, invocation.clone())
            .await;
        self.complete_invocation(&invocation, &result);
        result.map(McpJson)
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
        let invocation = self.begin_invocation("apply_patch", &input, &request_meta, None)?;
        let result = self.execute_apply_patch(input, invocation.clone()).await;
        self.complete_invocation(&invocation, &result);
        result
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
    let invocation_recorder = policy.invocation_recorder.clone();
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
                invocation_recorder.clone(),
            ))
        },
        LocalSessionManager::default().into(),
        config,
    )
}

fn build_app(config: Arc<Config>, mcp_service: McpHttpService) -> Router {
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
        .route_layer(middleware::from_fn_with_state(config, query_key_auth));

    Router::new()
        .route("/health", get(health))
        .merge(protected_mcp_router)
}

pub async fn run_server(config: Config) -> Result<()> {
    let bind = format!("{}:{}", config.bind_host, config.service_port);
    let addr: std::net::SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid bind address {bind}"))?;

    let config = Arc::new(config);
    let zodex_service = ZodexService::new(config.clone());

    let cancellation = CancellationToken::new();
    let mcp_service = build_mcp_service(zodex_service.clone(), cancellation.child_token());
    let app = build_app(config, mcp_service);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind Sprite HTTP listener on {addr}"))?;
    let shutdown = cancellation.clone();

    info!("zodexd listening on http://{bind}");
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown.cancel();
        })
        .await
        .context("Sprite HTTP server terminated unexpectedly")
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "component": "zodexd",
        "version": env!("CARGO_PKG_VERSION")
    }))
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
mod http_surface_tests;
#[cfg(test)]
mod local_history_tests;
#[cfg(test)]
mod local_tests;
#[cfg(test)]
mod tests;
