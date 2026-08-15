use std::ffi::OsString;
use std::sync::{Arc, Mutex};

use reqwest::Response;
use serde_json::{Value, json};
use tempfile::tempdir;

use crate::config::Config;
use crate::protocol::{CommandStatus, TerminationReason, ToolOutput};
use crate::service::ZodexService;
use crate::session::{SessionOutputChunk, SessionOutputObserver, SessionRuntimePolicy};

use super::local::{
    LOCAL_MCP_TOKEN_HEADER, LocalMcpServerConfig, start_local_mcp_server_with_observer,
};
use super::start_local_mcp_server;

const TOKEN: &str = "phase4-local-mcp-token";

fn local_environment() -> Vec<(OsString, OsString)> {
    [
        ("HOME", "/tmp/zodex-local-test-home"),
        ("USER", "zodex-local-test"),
        ("LOGNAME", "zodex-local-test"),
        ("PATH", "/usr/bin:/bin"),
    ]
    .into_iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect()
}

fn local_service(config: Config) -> ZodexService {
    ZodexService::with_session_policy(
        Arc::new(config),
        SessionRuntimePolicy::local("/bin/sh", local_environment()).unwrap(),
    )
}

fn test_http_client() -> reqwest::Client {
    crate::install_rustls_crypto_provider();
    reqwest::Client::new()
}

fn modern_meta(openai_session: Option<&str>) -> Value {
    let mut meta = serde_json::Map::from_iter([
        (
            "io.modelcontextprotocol/protocolVersion".to_string(),
            json!("2026-07-28"),
        ),
        (
            "io.modelcontextprotocol/clientInfo".to_string(),
            json!({"name": "zodex-phase4-local-test", "version": "1.0"}),
        ),
        (
            "io.modelcontextprotocol/clientCapabilities".to_string(),
            json!({}),
        ),
    ]);
    if let Some(session) = openai_session {
        meta.insert("openai/session".to_string(), json!(session));
    }
    Value::Object(meta)
}

async fn post_mcp(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
    method_header: Option<&str>,
    body: Value,
) -> Response {
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .json(&body);
    if let Some(token) = token {
        request = request.header(LOCAL_MCP_TOKEN_HEADER, token);
    }
    if let Some(method) = method_header {
        request = request
            .header("MCP-Protocol-Version", "2026-07-28")
            .header("Mcp-Method", method);
        if method == "tools/call"
            && let Some(name) = body
                .get("params")
                .and_then(|params| params.get("name"))
                .and_then(Value::as_str)
        {
            request = request.header("Mcp-Name", name);
        }
    }
    request.send().await.unwrap()
}

async fn json_response(response: Response) -> Value {
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    assert!(
        status.is_success(),
        "unexpected HTTP status {status}: {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

fn tool_output(value: &Value) -> ToolOutput {
    serde_json::from_value(value["result"]["structuredContent"].clone())
        .unwrap_or_else(|error| panic!("invalid ToolOutput in MCP response ({error}): {value}"))
}

fn tool_call(id: u64, name: &str, arguments: Value, session: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments,
            "_meta": modern_meta(Some(session)),
        }
    })
}

#[tokio::test]
async fn local_listener_is_loopback_token_authenticated_and_exposes_only_mcp_surface() {
    let dir = tempdir().unwrap();
    let server = start_local_mcp_server(
        local_service(Config::default()),
        LocalMcpServerConfig::new(dir.path(), TOKEN),
    )
    .await
    .unwrap();
    assert!(server.addr().ip().is_loopback());
    let client = test_http_client();
    let discover = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "server/discover",
        "params": {"_meta": modern_meta(None)}
    });

    for token in [None, Some("wrong-token")] {
        let response = post_mcp(
            &client,
            &server.url(),
            token,
            Some("server/discover"),
            discover.clone(),
        )
        .await;
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    let response = client
        .post(server.url())
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "server/discover")
        .json(&discover)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

    let response = client
        .post(format!("http://{}/v1/exec-command", server.addr()))
        .header(LOCAL_MCP_TOKEN_HEADER, TOKEN)
        .json(&json!({"cmd":"echo no", "workdir": dir.path()}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn local_discovery_and_tools_list_are_stateless_and_runtime_specific() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let client = test_http_client();

    for (id, dir) in [(10_u64, first.path()), (20_u64, second.path())] {
        let server = start_local_mcp_server(
            local_service(Config::default()),
            LocalMcpServerConfig::new(dir, TOKEN),
        )
        .await
        .unwrap();
        let discover = json_response(
            post_mcp(
                &client,
                &server.url(),
                Some(TOKEN),
                Some("server/discover"),
                json!({
                    "jsonrpc":"2.0",
                    "id": id,
                    "method":"server/discover",
                    "params":{"_meta": modern_meta(None)}
                }),
            )
            .await,
        )
        .await;
        assert!(
            discover["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains(dir.to_string_lossy().as_ref())
        );
        assert!(discover["result"]["instructions"].as_str().unwrap().contains(
            "Every exec_command and apply_patch call must still provide an absolute existing workdir"
        ));

        let listed = json_response(
            post_mcp(
                &client,
                &server.url(),
                Some(TOKEN),
                Some("tools/list"),
                json!({
                    "jsonrpc":"2.0",
                    "id": id + 1,
                    "method":"tools/list",
                    "params":{"_meta": modern_meta(None)}
                }),
            )
            .await,
        )
        .await;
        let mut names = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["apply_patch", "exec_command", "write_stdin"]);
        assert!(
            listed["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|tool| matches!(
                    tool["name"].as_str(),
                    Some("exec_command" | "apply_patch")
                ))
                .all(|tool| tool["inputSchema"]["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| entry == "workdir"))
        );

        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn local_compat_initialize_is_sessionless_but_legacy_tool_traffic_is_not_a_fallback() {
    let dir = tempdir().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer_seen = seen.clone();
    let observer: super::ProviderMetadataObserver = Arc::new(move |metadata| {
        observer_seen.lock().unwrap().push(metadata.clone());
    });
    let server = start_local_mcp_server_with_observer(
        local_service(Config::default()),
        LocalMcpServerConfig::new(dir.path(), TOKEN),
        Some(observer),
    )
    .await
    .unwrap();
    let client = test_http_client();

    let initialize = post_mcp(
        &client,
        &server.url(),
        Some(TOKEN),
        None,
        json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "protocolVersion":"2025-06-18",
                "capabilities":{},
                "clientInfo":{"name":"tunnel-probe","version":"1"}
            }
        }),
    )
    .await;
    assert_eq!(initialize.status(), reqwest::StatusCode::OK);
    assert!(initialize.headers().get("Mcp-Session-Id").is_none());
    let initialized: Value = initialize.json().await.unwrap();
    assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18");
    assert!(seen.lock().unwrap().is_empty());

    let legacy_tool = post_mcp(
        &client,
        &server.url(),
        Some(TOKEN),
        None,
        json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"tools/call",
            "params":{
                "name":"exec_command",
                "arguments":{"cmd":"echo legacy-must-not-run","workdir":dir.path()}
            }
        }),
    )
    .await;
    let status = legacy_tool.status();
    let body = legacy_tool.text().await.unwrap();
    assert!(
        !status.is_success() || body.contains("error"),
        "legacy tool traffic unexpectedly became a Local fallback: {status} {body}"
    );
    assert!(seen.lock().unwrap().is_empty());
    server.shutdown().await.unwrap();
}

#[derive(Default)]
struct InvocationCapture {
    chunks: Mutex<Vec<SessionOutputChunk>>,
}

impl SessionOutputObserver for InvocationCapture {
    fn observe_output(&self, chunk: SessionOutputChunk) {
        self.chunks.lock().unwrap().push(chunk);
    }
}

#[tokio::test]
async fn local_http_exercises_all_three_tools_concurrently_and_preserves_provider_context() {
    let root = tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let capture = Arc::new(InvocationCapture::default());
    let policy = SessionRuntimePolicy::local("/bin/sh", local_environment())
        .unwrap()
        .with_output_observer(capture.clone());
    let service = ZodexService::with_session_policy(Arc::new(Config::default()), policy);
    let server = start_local_mcp_server(
        service.clone(),
        LocalMcpServerConfig::new(root.path(), TOKEN),
    )
    .await
    .unwrap();
    let client = test_http_client();

    let call = |id, dir: &std::path::Path, marker: &'static str, session: &'static str| {
        let client = client.clone();
        let url = server.url();
        let dir = dir.to_path_buf();
        async move {
            json_response(
                post_mcp(
                    &client,
                    &url,
                    Some(TOKEN),
                    Some("tools/call"),
                    tool_call(
                        id,
                        "exec_command",
                        json!({
                            "cmd": format!("printf '{marker}\\n'; pwd"),
                            "workdir": dir,
                            "yield_time_ms": 2000
                        }),
                        session,
                    ),
                )
                .await,
            )
            .await
        }
    };
    let (out_a, out_b) = tokio::join!(
        call(10, &a, "from-a", "session-a"),
        call(11, &b, "from-b", "session-b")
    );
    let out_a = tool_output(&out_a);
    let out_b = tool_output(&out_b);
    assert_eq!(out_a.status, CommandStatus::Exited);
    assert_eq!(out_b.status, CommandStatus::Exited);
    assert_eq!(out_a.cwd, a.display().to_string());
    assert_eq!(out_b.cwd, b.display().to_string());
    assert!(out_a.output.contains("from-a"));
    assert!(out_b.output.contains("from-b"));

    let patch_path = root.path().join("patched.txt");
    let patched = json_response(
        post_mcp(
            &client,
            &server.url(),
            Some(TOKEN),
            Some("tools/call"),
            tool_call(
                12,
                "apply_patch",
                json!({
                    "patch":"*** Begin Patch\n*** Add File: patched.txt\n+local-mcp-patch\n*** End Patch\n",
                    "workdir":root.path()
                }),
                "session-a",
            ),
        )
        .await,
    )
    .await;
    assert!(
        patched.get("error").is_none(),
        "unexpected patch response: {patched}"
    );
    assert_eq!(
        std::fs::read_to_string(patch_path).unwrap(),
        "local-mcp-patch\n"
    );

    let started = json_response(
        post_mcp(
            &client,
            &server.url(),
            Some(TOKEN),
            Some("tools/call"),
            tool_call(
                13,
                "exec_command",
                json!({"cmd":"sleep 30","workdir":root.path(),"yield_time_ms":50,"timeout_ms":60000}),
                "session-a",
            ),
        )
        .await,
    )
    .await;
    let started = tool_output(&started);
    assert_eq!(started.status, CommandStatus::Running);
    let handle = started.session_handle.unwrap();
    let killed = json_response(
        post_mcp(
            &client,
            &server.url(),
            Some(TOKEN),
            Some("tools/call"),
            tool_call(
                14,
                "write_stdin",
                json!({"session_handle":handle,"kill_process":true,"yield_time_ms":6000}),
                "session-b",
            ),
        )
        .await,
    )
    .await;
    let killed = tool_output(&killed);
    assert_eq!(killed.status, CommandStatus::Exited);
    assert_eq!(killed.termination_reason, Some(TerminationReason::Killed));

    let timed_out = json_response(
        post_mcp(
            &client,
            &server.url(),
            Some(TOKEN),
            Some("tools/call"),
            tool_call(
                15,
                "exec_command",
                json!({"cmd":"sleep 30","workdir":root.path(),"yield_time_ms":2500,"timeout_ms":1000}),
                "session-a",
            ),
        )
        .await,
    )
    .await;
    let timed_out = tool_output(&timed_out);
    assert_eq!(timed_out.status, CommandStatus::Exited);
    assert_eq!(
        timed_out.termination_reason,
        Some(TerminationReason::Timeout)
    );

    {
        let chunks = capture.chunks.lock().unwrap();
        let sessions = chunks
            .iter()
            .filter_map(|chunk| chunk.invocation.provider.as_ref())
            .map(|provider| provider.session_key.to_string())
            .collect::<std::collections::HashSet<_>>();
        assert!(sessions.contains("session-a"));
        assert!(sessions.contains("session-b"));
        assert!(
            chunks
                .iter()
                .filter_map(|chunk| chunk.invocation.correlation_id.as_deref())
                .all(|id| id.len() == 32)
        );
    }

    server.shutdown().await.unwrap();
    service.shutdown_sessions().await.unwrap();
}
