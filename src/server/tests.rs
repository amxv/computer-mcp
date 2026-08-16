use super::{
    McpServerPolicy, ProviderMetadata, ProviderMetadataObserver, ZodexMcpService,
    build_mcp_service_with_policy, extract_provider_metadata, key_from_query,
    rewrite_mcp_transport_root_uri,
};
use crate::config::Config;
use crate::protocol::{
    ApplyPatchInput, CommandStatus, ExecCommandInput, TerminationReason, ToolOutput,
    WriteStdinInput,
};
use crate::service::ZodexService;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, Uri};
use rmcp::ServerHandler;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{RequestMetaObject, ToolAnnotations};
use serde_json::json;
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

fn test_config() -> Arc<Config> {
    Arc::new(Config::default())
}

fn test_workdir() -> String {
    std::env::current_dir()
        .expect("test current directory")
        .to_string_lossy()
        .to_string()
}

fn stateless_policy(observer: Option<ProviderMetadataObserver>) -> McpServerPolicy {
    McpServerPolicy {
        legacy_session_mode: false,
        json_response: true,
        stateless_protocol_metadata_required: true,
        instructions: Arc::from("phase-1 stateless test server"),
        provider_metadata_observer: observer,
    }
}

async fn spawn_stateless_mcp(
    policy: McpServerPolicy,
) -> (reqwest::Client, String, CancellationToken, JoinHandle<()>) {
    crate::install_rustls_crypto_provider();
    let cancellation = CancellationToken::new();
    let service = ZodexService::new(test_config());
    let mcp = build_mcp_service_with_policy(service, cancellation.child_token(), policy);
    let app = axum::Router::new().nest_service("/mcp", mcp);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stateless MCP test listener");
    let addr = listener.local_addr().expect("stateless MCP test addr");
    let shutdown = cancellation.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .expect("stateless MCP test server");
    });
    (
        reqwest::Client::new(),
        format!("http://{addr}/mcp"),
        cancellation,
        task,
    )
}

async fn wait_for_service_exit(service: &ZodexService, mut output: ToolOutput) -> ToolOutput {
    for _ in 0..10 {
        if output.status == CommandStatus::Exited {
            return output;
        }

        output = service
            .write_stdin(WriteStdinInput {
                session_handle: output
                    .session_handle
                    .expect("running output should have a session handle"),
                chars: None,
                yield_time_ms: Some(250),
                kill_process: Some(false),
            })
            .await
            .expect("service poll should succeed");
    }

    panic!("service output did not reach exited state in time");
}

async fn wait_for_mcp_exit(mcp: &ZodexMcpService, mut output: ToolOutput) -> ToolOutput {
    for _ in 0..10 {
        if output.status == CommandStatus::Exited {
            return output;
        }

        output = mcp
            .write_stdin(
                Parameters(WriteStdinInput {
                    session_handle: output
                        .session_handle
                        .expect("running output should have a session handle"),
                    chars: None,
                    yield_time_ms: Some(250),
                    kill_process: Some(false),
                }),
                RequestMetaObject::default(),
            )
            .await
            .expect("mcp poll should succeed")
            .0;
    }

    panic!("mcp output did not reach exited state in time");
}

#[test]
fn registers_apply_patch_tool() {
    let service = ZodexMcpService::new(ZodexService::new(test_config()));
    let names: Vec<String> = service
        .tool_router
        .list_all()
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();

    assert!(names.iter().any(|name| name == "exec_command"));
    assert!(names.iter().any(|name| name == "write_stdin"));
    assert!(names.iter().any(|name| name == "apply_patch"));
    assert!(
        names.iter().all(|name| name != "publish-pr"),
        "publish-pr must not be exposed on remote MCP surface"
    );
    assert!(
        names.iter().all(|name| name != "publish_pr"),
        "publish_pr must not be exposed on remote MCP surface"
    );
}

#[test]
fn server_info_mentions_zodex_remote_execution_tools() {
    let service = ZodexMcpService::new(ZodexService::new(test_config()));
    let info = service.get_info();
    assert_eq!(
        info.instructions.as_deref().unwrap_or_default(),
        "zodex remote execution tools"
    );
}

#[test]
fn server_info_accepts_runtime_supplied_workdir_guidance() {
    let instructions = Arc::<str>::from(
        "runtime start directory: /tmp/example; use it as the suggested initial explicit workdir; every command/patch must still send an absolute workdir",
    );
    let service =
        ZodexMcpService::with_options(ZodexService::new(test_config()), instructions.clone(), None);

    assert_eq!(
        service.get_info().instructions.as_deref(),
        Some(instructions.as_ref())
    );
}

#[test]
fn provider_metadata_extracts_openai_session_without_tool_schema_bookkeeping() {
    let meta: RequestMetaObject = serde_json::from_value(json!({
        "openai/session": "opaque-chatgpt-conversation"
    }))
    .expect("request metadata should deserialize");
    assert_eq!(
        extract_provider_metadata(&meta).openai_session.as_deref(),
        Some("opaque-chatgpt-conversation")
    );

    let malformed: RequestMetaObject = serde_json::from_value(json!({
        "openai/session": 42
    }))
    .expect("request metadata should deserialize");
    assert_eq!(extract_provider_metadata(&malformed).openai_session, None);

    let service = ZodexMcpService::new(ZodexService::new(test_config()));
    for tool in service.tool_router.list_all() {
        let schema = tool.input_schema.as_ref();
        let properties = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("tool input schema properties");
        for forbidden in [
            "agent_id",
            "conversation_id",
            "runtime_id",
            "openai/session",
        ] {
            assert!(
                !properties.contains_key(forbidden),
                "{forbidden} must stay outside model-visible tool arguments"
            );
        }
    }
}

#[test]
fn workdir_is_required_in_model_visible_exec_and_patch_schemas() {
    let service = ZodexMcpService::new(ZodexService::new(test_config()));
    let tools = service.tool_router.list_all();
    for name in ["exec_command", "apply_patch"] {
        let tool = tools
            .iter()
            .find(|tool| tool.name == name)
            .expect("tool should be registered");
        let required = tool
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .expect("required schema list");
        assert!(
            required.iter().any(|entry| entry == "workdir"),
            "{name}.workdir must be required"
        );
        let workdir_schema = tool
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .and_then(|properties| properties.get("workdir"))
            .expect("workdir property schema");
        assert!(
            workdir_schema.get("default").is_none(),
            "{name}.workdir must not advertise a backend default"
        );
    }
}

#[test]
fn tools_have_expected_annotations() {
    let service = ZodexMcpService::new(ZodexService::new(test_config()));

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

#[tokio::test]
async fn exec_command_mcp_parity_with_service() {
    let config = test_config();
    let direct = ZodexService::new(config.clone());
    let mcp = ZodexMcpService::new(ZodexService::new(config));
    let input = ExecCommandInput {
        cmd: "printf 'mcp-exec\\n'".to_string(),
        yield_time_ms: Some(2_000),
        workdir: test_workdir(),
        timeout_ms: None,
    };

    let direct_output = wait_for_service_exit(
        &direct,
        direct
            .exec_command(input.clone())
            .await
            .expect("direct service exec should succeed"),
    )
    .await;
    let mcp_output = wait_for_mcp_exit(
        &mcp,
        mcp.exec_command(Parameters(input), RequestMetaObject::default())
            .await
            .expect("mcp exec should succeed")
            .0,
    )
    .await;

    assert_eq!(mcp_output.status, direct_output.status);
    assert_eq!(mcp_output.exit_code, direct_output.exit_code);
    assert_eq!(
        mcp_output.termination_reason,
        direct_output.termination_reason
    );
    assert!(mcp_output.output.contains("mcp-exec"));
    assert!(direct_output.output.contains("mcp-exec"));
}

#[tokio::test]
async fn write_stdin_mcp_parity_with_service() {
    let config = test_config();
    let direct = ZodexService::new(config.clone());
    let mcp = ZodexMcpService::new(ZodexService::new(config));
    let shell_input = ExecCommandInput {
        cmd: "bash --noprofile --norc".to_string(),
        yield_time_ms: Some(50),
        workdir: test_workdir(),
        timeout_ms: Some(60_000),
    };

    let direct_started = direct
        .exec_command(shell_input.clone())
        .await
        .expect("direct shell should start");
    let mcp_started = mcp
        .exec_command(Parameters(shell_input), RequestMetaObject::default())
        .await
        .expect("mcp shell should start")
        .0;

    let direct_session_handle = direct_started
        .session_handle
        .expect("direct shell should have a session handle");
    let mcp_session_handle = mcp_started
        .session_handle
        .expect("mcp shell should have a session handle");

    let direct_write = direct
        .write_stdin(WriteStdinInput {
            session_handle: direct_session_handle.clone(),
            chars: Some("echo mcp-write\n".to_string()),
            yield_time_ms: Some(500),
            kill_process: Some(false),
        })
        .await
        .expect("direct write should succeed");
    let mcp_write = mcp
        .write_stdin(
            Parameters(WriteStdinInput {
                session_handle: mcp_session_handle.clone(),
                chars: Some("echo mcp-write\n".to_string()),
                yield_time_ms: Some(500),
                kill_process: Some(false),
            }),
            RequestMetaObject::default(),
        )
        .await
        .expect("mcp write should succeed")
        .0;

    assert_eq!(mcp_write.status, direct_write.status);
    assert_eq!(
        mcp_write.termination_reason,
        direct_write.termination_reason
    );
    assert_eq!(mcp_write.status, CommandStatus::Running);
    assert!(mcp_write.output.contains("mcp-write"));
    assert!(direct_write.output.contains("mcp-write"));

    let _ = direct
        .write_stdin(WriteStdinInput {
            session_handle: direct_session_handle,
            chars: Some("exit\n".to_string()),
            yield_time_ms: Some(2_000),
            kill_process: Some(false),
        })
        .await
        .expect("direct shell should exit");
    let _ = mcp
        .write_stdin(
            Parameters(WriteStdinInput {
                session_handle: mcp_session_handle,
                chars: Some("exit\n".to_string()),
                yield_time_ms: Some(2_000),
                kill_process: Some(false),
            }),
            RequestMetaObject::default(),
        )
        .await
        .expect("mcp shell should exit");
}

#[tokio::test]
async fn kill_process_mcp_parity_with_service() {
    let config = test_config();
    let direct = ZodexService::new(config.clone());
    let mcp = ZodexMcpService::new(ZodexService::new(config));
    let input = ExecCommandInput {
        cmd: "sleep 30".to_string(),
        yield_time_ms: Some(50),
        workdir: test_workdir(),
        timeout_ms: Some(60_000),
    };

    let direct_started = direct
        .exec_command(input.clone())
        .await
        .expect("direct sleep should start");
    let mcp_started = mcp
        .exec_command(Parameters(input), RequestMetaObject::default())
        .await
        .expect("mcp sleep should start")
        .0;

    let direct_killed = direct
        .write_stdin(WriteStdinInput {
            session_handle: direct_started
                .session_handle
                .expect("direct running handle"),
            chars: Some("echo ignored-direct\n".to_string()),
            yield_time_ms: Some(6_000),
            kill_process: Some(true),
        })
        .await
        .expect("direct kill should succeed");
    let mcp_killed = mcp
        .write_stdin(
            Parameters(WriteStdinInput {
                session_handle: mcp_started.session_handle.expect("mcp running handle"),
                chars: Some("echo ignored-mcp\n".to_string()),
                yield_time_ms: Some(6_000),
                kill_process: Some(true),
            }),
            RequestMetaObject::default(),
        )
        .await
        .expect("mcp kill should succeed")
        .0;

    assert_eq!(mcp_killed.status, direct_killed.status);
    assert_eq!(
        mcp_killed.termination_reason,
        direct_killed.termination_reason
    );
    assert!(mcp_killed.session_handle.is_none());
    assert!(direct_killed.session_handle.is_none());
    assert!(mcp_killed.output.contains("terminated by kill_process"));
    assert!(direct_killed.output.contains("terminated by kill_process"));
    assert!(!mcp_killed.output.contains("ignored-mcp"));
    assert!(!direct_killed.output.contains("ignored-direct"));
}

#[tokio::test]
async fn timeout_and_cwd_mcp_parity_with_service() {
    let config = Arc::new(Config {
        default_exec_timeout_ms: 1_000,
        max_exec_timeout_ms: 1_000,
        ..Config::default()
    });
    let direct = ZodexService::new(config.clone());
    let mcp = ZodexMcpService::new(ZodexService::new(config));
    let dir = tempdir().expect("tempdir");

    let direct_cwd = direct
        .exec_command(ExecCommandInput {
            cmd: "pwd".to_string(),
            yield_time_ms: Some(2_000),
            workdir: dir.path().to_string_lossy().to_string(),
            timeout_ms: None,
        })
        .await
        .expect("direct cwd should succeed");
    let mcp_cwd = mcp
        .exec_command(
            Parameters(ExecCommandInput {
                cmd: "pwd".to_string(),
                yield_time_ms: Some(2_000),
                workdir: dir.path().to_string_lossy().to_string(),
                timeout_ms: None,
            }),
            RequestMetaObject::default(),
        )
        .await
        .expect("mcp cwd should succeed")
        .0;

    assert_eq!(mcp_cwd.cwd, direct_cwd.cwd);
    assert!(
        mcp_cwd
            .output
            .contains(dir.path().to_string_lossy().as_ref())
    );
    assert!(
        direct_cwd
            .output
            .contains(dir.path().to_string_lossy().as_ref())
    );

    let direct_timeout = direct
        .exec_command(ExecCommandInput {
            cmd: "sleep 30".to_string(),
            yield_time_ms: Some(2_500),
            workdir: test_workdir(),
            timeout_ms: Some(1_000),
        })
        .await
        .expect("direct timeout should complete");
    let mcp_timeout = mcp
        .exec_command(
            Parameters(ExecCommandInput {
                cmd: "sleep 30".to_string(),
                yield_time_ms: Some(2_500),
                workdir: test_workdir(),
                timeout_ms: Some(1_000),
            }),
            RequestMetaObject::default(),
        )
        .await
        .expect("mcp timeout should complete")
        .0;

    assert_eq!(mcp_timeout.status, direct_timeout.status);
    assert_eq!(
        mcp_timeout.termination_reason,
        direct_timeout.termination_reason
    );
    assert_eq!(
        mcp_timeout.termination_reason,
        Some(TerminationReason::Timeout)
    );
    assert!(
        mcp_timeout
            .output
            .contains("process timed out and was terminated")
    );
    assert!(
        direct_timeout
            .output
            .contains("process timed out and was terminated")
    );
}

#[tokio::test]
async fn apply_patch_mcp_parity_with_service() {
    let config = test_config();
    let direct = ZodexService::new(config.clone());
    let mcp = ZodexMcpService::new(ZodexService::new(config));
    let direct_dir = tempdir().expect("direct tempdir");
    let mcp_dir = tempdir().expect("mcp tempdir");
    let patch = "*** Begin Patch\n*** Add File: parity.txt\n+mcp-patch\n*** End Patch\n";

    let direct_output = direct
        .apply_patch(ApplyPatchInput {
            patch: patch.to_string(),
            workdir: direct_dir.path().to_string_lossy().to_string(),
        })
        .expect("direct apply_patch should succeed");
    let mcp_output = mcp
        .apply_patch(
            Parameters(ApplyPatchInput {
                patch: patch.to_string(),
                workdir: mcp_dir.path().to_string_lossy().to_string(),
            }),
            RequestMetaObject::default(),
        )
        .await
        .expect("mcp apply_patch should succeed");

    assert!(direct_output.contains("Success. Updated the following files:"));
    assert!(mcp_output.contains("Success. Updated the following files:"));
    assert_eq!(
        fs::read_to_string(direct_dir.path().join("parity.txt")).expect("read direct patch"),
        "mcp-patch\n"
    );
    assert_eq!(
        fs::read_to_string(mcp_dir.path().join("parity.txt")).expect("read mcp patch"),
        "mcp-patch\n"
    );
}

#[tokio::test]
async fn modern_stateless_tool_call_observes_openai_session_without_transport_session() {
    let seen = Arc::new(Mutex::new(Vec::<ProviderMetadata>::new()));
    let observer_seen = seen.clone();
    let observer: Arc<dyn Fn(&ProviderMetadata) + Send + Sync> = Arc::new(move |metadata| {
        observer_seen
            .lock()
            .expect("provider metadata lock")
            .push(metadata.clone());
    });
    let (client, url, cancellation, server) =
        spawn_stateless_mcp(stateless_policy(Some(observer))).await;
    let dir = tempdir().expect("tempdir");
    let body = json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "tools/call",
        "params": {
            "name": "exec_command",
            "arguments": {
                "cmd": "pwd",
                "workdir": dir.path(),
                "yield_time_ms": 2_000
            },
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientInfo": {
                    "name": "zodex-phase-1-test",
                    "version": "1.0"
                },
                "io.modelcontextprotocol/clientCapabilities": {},
                "openai/session": "opaque-phase-1-session"
            }
        }
    });

    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/call")
        .header("Mcp-Name", "exec_command")
        .json(&body)
        .send()
        .await
        .expect("modern stateless tool call");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(
        response.headers().get("Mcp-Session-Id").is_none(),
        "stateless tool call must not create a transport session"
    );
    let value: serde_json::Value = response.json().await.expect("tool call JSON response");
    assert_eq!(value["id"], 41);
    assert!(
        value.get("error").is_none(),
        "unexpected MCP error: {value}"
    );
    assert_eq!(
        *seen.lock().expect("provider metadata lock"),
        vec![ProviderMetadata {
            openai_session: Some("opaque-phase-1-session".to_string())
        }]
    );

    cancellation.cancel();
    server.await.expect("stateless server task");
}

#[tokio::test]
async fn modern_discover_publishes_runtime_workdir_guidance_without_session_state() {
    let instructions = "runtime start directory: /tmp/zodex-runtime-start; use it as the suggested initial explicit workdir; every command/patch must still send an absolute workdir";
    let policy = McpServerPolicy {
        instructions: Arc::from(instructions),
        ..stateless_policy(None)
    };
    let (client, url, cancellation, server) = spawn_stateless_mcp(policy).await;
    let body = json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "server/discover",
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientInfo": {
                    "name": "zodex-phase-1-discovery-test",
                    "version": "1.0"
                },
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    });

    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "server/discover")
        .json(&body)
        .send()
        .await
        .expect("modern stateless discovery request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(
        response.headers().get("Mcp-Session-Id").is_none(),
        "server/discover must not create a transport session"
    );
    let value: serde_json::Value = response.json().await.expect("discover JSON response");
    assert_eq!(value["id"], 42);
    assert_eq!(value["result"]["instructions"], instructions);
    assert!(
        value["result"]["supportedVersions"]
            .as_array()
            .expect("supported versions")
            .iter()
            .any(|version| version == "2026-07-28"),
        "discovery should advertise modern stateless MCP support: {value}"
    );

    cancellation.cancel();
    server.await.expect("stateless server task");
}

#[tokio::test]
async fn tunnel_compat_initialize_is_sessionless_and_has_no_provider_attribution() {
    let seen = Arc::new(Mutex::new(Vec::<ProviderMetadata>::new()));
    let observer_seen = seen.clone();
    let observer: Arc<dyn Fn(&ProviderMetadata) + Send + Sync> = Arc::new(move |metadata| {
        observer_seen
            .lock()
            .expect("provider metadata lock")
            .push(metadata.clone());
    });
    let (client, url, cancellation, server) =
        spawn_stateless_mcp(stateless_policy(Some(observer))).await;
    let body = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {
                "name": "tunnel-client-compatible-probe",
                "version": "1.0"
            }
        }
    });

    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .json(&body)
        .send()
        .await
        .expect("compatibility initialize");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(
        response.headers().get("Mcp-Session-Id").is_none(),
        "sessionless compatibility initialize must not create Mcp-Session-Id"
    );
    let value: serde_json::Value = response.json().await.expect("initialize JSON response");
    assert_eq!(value["result"]["protocolVersion"], "2025-06-18");
    assert!(
        seen.lock().expect("provider metadata lock").is_empty(),
        "initialize must not create provider attribution side effects"
    );

    cancellation.cancel();
    server.await.expect("stateless server task");
}

#[test]
fn key_from_query_extracts_key_value() {
    assert_eq!(
        key_from_query(Some("foo=1&key=expected-value&bar=2")),
        Some("expected-value".to_string())
    );
}

#[test]
fn key_from_query_rejects_missing_or_malformed_key() {
    assert_eq!(key_from_query(None), None);
    assert_eq!(key_from_query(Some("foo=1&bar=2")), None);
    assert_eq!(key_from_query(Some("foo=1&key&bar=2")), None);
}

#[test]
fn rewrite_mcp_transport_root_uri_rewrites_both_mcp_forms_preserving_query() {
    let uri: Uri = "/mcp?key=secret&x=1".parse().expect("uri parse");
    let rewritten = rewrite_mcp_transport_root_uri(&uri).expect("uri should rewrite");
    assert_eq!(rewritten.path(), "/");
    assert_eq!(rewritten.query(), Some("key=secret&x=1"));

    let slash_uri: Uri = "/mcp/?key=secret&x=1".parse().expect("uri parse");
    let slash_rewritten = rewrite_mcp_transport_root_uri(&slash_uri).expect("uri should rewrite");
    assert_eq!(slash_rewritten.path(), "/");
    assert_eq!(slash_rewritten.query(), Some("key=secret&x=1"));
}

#[test]
fn rewrite_mcp_transport_root_uri_skips_other_paths() {
    let uri: Uri = "/health".parse().expect("uri parse");
    assert_eq!(rewrite_mcp_transport_root_uri(&uri), None);
}

#[tokio::test]
async fn health_route_stays_public_and_stable() {
    let config = test_config();
    let service = ZodexService::new(config.clone());
    let app = super::build_app(
        config,
        super::build_mcp_service(service.clone(), CancellationToken::new()),
        service,
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .body(Body::empty())
                .expect("request build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(value, json!({ "status": "ok" }));
}

#[tokio::test]
async fn mcp_routes_accept_both_with_and_without_trailing_slash() {
    let config = test_config();
    let api_key = config.api_key.clone();
    let service = ZodexService::new(config.clone());
    let app = super::build_app(
        config,
        super::build_mcp_service(service.clone(), CancellationToken::new()),
        service,
    );
    let initialize_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "0.1"
            }
        }
    });

    for path in [
        format!("/mcp?key={api_key}"),
        format!("/mcp/?key={api_key}"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&path)
                    .header("host", "localhost")
                    .header("content-type", "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .body(Body::from(initialize_request.to_string()))
                    .expect("request build"),
            )
            .await
            .expect("request should succeed");

        let status = response.status();
        if status != StatusCode::OK {
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("failure body");
            panic!(
                "expected initialize to succeed for {path}; got {status}: {}",
                String::from_utf8_lossy(&body)
            );
        }
    }
}
