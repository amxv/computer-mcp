const MCP_COMPAT_PROTOCOL_VERSION: &str = "2025-06-18";
const MCP_MODERN_PROTOCOL_VERSION: &str = "2026-07-28";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpriteRuntimeHealth {
    component: String,
    version: String,
}

fn parse_sprite_runtime_health(raw: &str) -> Result<SpriteRuntimeHealth> {
    let value: serde_json::Value =
        serde_json::from_str(raw).context("failed to parse zodexd /health response")?;
    if value.get("status").and_then(serde_json::Value::as_str) != Some("ok") {
        bail!("zodexd /health did not report status=ok");
    }
    let component = value
        .get("component")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    if component != "zodexd" || version.is_empty() {
        bail!("zodexd /health is missing expected component/version identity");
    }
    Ok(SpriteRuntimeHealth { component, version })
}

fn read_local_sprite_runtime_health(sprite: &str, org: Option<&str>) -> Result<SpriteRuntimeHealth> {
    let args = vec![
        "curl".to_string(),
        "-fsS".to_string(),
        "--max-time".to_string(),
        "20".to_string(),
        "http://127.0.0.1:8080/health".to_string(),
    ];
    let raw = run_sprite_exec(sprite, org, &args, &[])?;
    parse_sprite_runtime_health(&raw)
}

fn verify_live_sprite_runtime_version(
    sprite: &str,
    org: Option<&str>,
    expected_version: &str,
) -> Result<()> {
    let health = read_local_sprite_runtime_health(sprite, org)?;
    validate_live_sprite_runtime_version(&health, expected_version)?;
    println!("sprite-live-runtime-version: {}", health.version);
    Ok(())
}

fn validate_live_sprite_runtime_version(
    health: &SpriteRuntimeHealth,
    expected_version: &str,
) -> Result<()> {
    if health.version != expected_version {
        bail!(
            "running zodexd version mismatch: expected `{expected_version}`, got `{}`. The binary may have been replaced without restarting the Sprite Service.",
            health.version
        );
    }
    Ok(())
}

fn verify_sprite_service_contract(
    sprite: &str,
    org: Option<&str>,
    config_path: &Path,
) -> Result<()> {
    let services = fetch_sprite_services(sprite, org)?;
    ensure_sprite_services_running(
        &services,
        &[PUBLISHER_SERVICE_LABEL, SPRITE_MAIN_SERVICE_LABEL],
    )?;
    let expected = expected_sprite_service_definitions(config_path);
    for service_name in [PUBLISHER_SERVICE_LABEL, SPRITE_MAIN_SERVICE_LABEL] {
        let actual = services
            .iter()
            .find(|service| service.name == service_name)
            .ok_or_else(|| anyhow!("Sprite Service {service_name} is missing"))?;
        let definition = expected
            .get(service_name)
            .ok_or_else(|| anyhow!("expected Sprite Service definition is missing"))?;
        if !sprite_service_matches_definition(actual, definition) {
            bail!(
                "Sprite Service {service_name} definition is stale; run `zodex sprite sync --sprite {sprite}`"
            );
        }
    }
    Ok(())
}

fn validate_mcp_tool_contract(value: &serde_json::Value) -> Result<()> {
    let tools = value
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow!("MCP tools/list response is missing result.tools"))?;
    let mut names = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    names.sort_unstable();
    if names != ["apply_patch", "exec_command", "write_stdin"] {
        bail!("MCP tool set drifted; expected exactly apply_patch, exec_command, write_stdin");
    }

    let expected = [
        (
            "exec_command",
            ["cmd", "timeout_ms", "workdir", "yield_time_ms"].as_slice(),
            ["cmd", "workdir"].as_slice(),
        ),
        (
            "write_stdin",
            ["chars", "kill_process", "session_handle", "yield_time_ms"].as_slice(),
            ["session_handle"].as_slice(),
        ),
        (
            "apply_patch",
            ["patch", "workdir"].as_slice(),
            ["patch", "workdir"].as_slice(),
        ),
    ];
    for (name, expected_properties, expected_required) in expected {
        let tool = tools
            .iter()
            .find(|tool| tool.get("name").and_then(serde_json::Value::as_str) == Some(name))
            .ok_or_else(|| anyhow!("MCP tool {name} is missing"))?;
        let schema = tool
            .get("inputSchema")
            .ok_or_else(|| anyhow!("MCP tool {name} is missing inputSchema"))?;
        let properties = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| anyhow!("MCP tool {name} inputSchema is missing properties"))?;
        let mut property_names = properties.keys().map(String::as_str).collect::<Vec<_>>();
        property_names.sort_unstable();
        let mut expected_properties = expected_properties.to_vec();
        expected_properties.sort_unstable();
        if property_names != expected_properties {
            bail!("MCP tool {name} property schema drifted");
        }
        let mut required = schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow!("MCP tool {name} inputSchema is missing required"))?
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        required.sort_unstable();
        let mut expected_required = expected_required.to_vec();
        expected_required.sort_unstable();
        if required != expected_required {
            bail!("MCP tool {name} required schema drifted");
        }
        if matches!(name, "exec_command" | "apply_patch")
            && properties
                .get("workdir")
                .and_then(|workdir| workdir.get("default"))
                .is_some()
        {
            bail!("MCP tool {name}.workdir unexpectedly advertises a default");
        }
    }
    Ok(())
}

async fn post_worker_mcp(
    client: &reqwest::Client,
    capability_url: &str,
    method_header: Option<&str>,
    body: serde_json::Value,
) -> Result<serde_json::Value> {
    let expected_id = body.get("id").cloned();
    let mut request = client
        .post(capability_url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(method) = method_header {
        request = request
            .header("MCP-Protocol-Version", MCP_MODERN_PROTOCOL_VERSION)
            .header("Mcp-Method", method);
    }
    let response = request
        .json(&body)
        .send()
        .await
        .map_err(|_| anyhow!("Worker MCP request failed; capability URL suppressed"))?;
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let text = response
        .text()
        .await
        .map_err(|_| anyhow!("Worker MCP response could not be read"))?;
    if !status.is_success() {
        bail!("Worker MCP request returned HTTP {status}");
    }
    parse_worker_mcp_response(content_type.as_deref(), &text, expected_id.as_ref())
}

fn parse_worker_mcp_response(
    content_type: Option<&str>,
    body: &str,
    expected_id: Option<&serde_json::Value>,
) -> Result<serde_json::Value> {
    let is_event_stream = content_type
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"));

    if !is_event_stream {
        return serde_json::from_str(body).context("Worker MCP response was not valid JSON");
    }

    let mut data_lines = Vec::new();
    let mut saw_json_message = false;

    for raw_line in body.lines().chain(std::iter::once("")) {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            if data_lines.is_empty() {
                continue;
            }

            let event_data = data_lines.join("\n");
            data_lines.clear();
            if event_data.trim().is_empty() {
                continue;
            }

            let Ok(value) = serde_json::from_str::<serde_json::Value>(&event_data) else {
                continue;
            };
            saw_json_message = true;
            if expected_id.is_none_or(|id| value.get("id") == Some(id)) {
                return Ok(value);
            }
            continue;
        }

        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.strip_prefix(' ').unwrap_or(data).to_string());
        }
    }

    if saw_json_message {
        bail!("Worker MCP event stream did not contain the expected JSON-RPC response");
    }
    bail!("Worker MCP event stream did not contain a JSON-RPC response")
}

async fn verify_worker_mcp_contract(worker_url: &str, key: &str) -> Result<()> {
    let capability_url = build_sprite_connect_url(worker_url, key)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build MCP verification client")?;
    let initialize = post_worker_mcp(
        &client,
        &capability_url,
        None,
        serde_json::json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "protocolVersion":MCP_COMPAT_PROTOCOL_VERSION,
                "capabilities":{},
                "clientInfo":{"name":"zodex-setup-verify","version":env!("CARGO_PKG_VERSION")}
            }
        }),
    )
    .await?;
    if initialize
        .get("result")
        .and_then(|result| result.get("protocolVersion"))
        .and_then(serde_json::Value::as_str)
        != Some(MCP_COMPAT_PROTOCOL_VERSION)
    {
        bail!("Worker MCP initialize compatibility contract drifted");
    }

    let listed = post_worker_mcp(
        &client,
        &capability_url,
        Some("tools/list"),
        serde_json::json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"tools/list",
            "params":{"_meta":{
                "io.modelcontextprotocol/protocolVersion":MCP_MODERN_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientInfo":{
                    "name":"zodex-setup-verify",
                    "version":env!("CARGO_PKG_VERSION")
                },
                "io.modelcontextprotocol/clientCapabilities":{}
            }}
        }),
    )
    .await?;
    validate_mcp_tool_contract(&listed)?;
    Ok(())
}

async fn verify_sprite_end_to_end_health(
    resolved: &ResolvedSprite,
    record: &OperatorSpriteRecord,
) -> Result<()> {
    let config_path = Path::new(&record.remote_config);
    verify_sprite_service_contract(&resolved.name, resolved.org.as_deref(), config_path)
        .context("service/process layer failed")?;
    let local_health = read_local_sprite_runtime_health(&resolved.name, resolved.org.as_deref())
        .context("localhost runtime layer failed")?;
    println!("health-services: ok");
    println!("health-localhost: ok ({})", local_health.version);

    let info = sprite_url_info(&resolved.name, resolved.org.as_deref())?;
    if info.auth.as_deref() != Some("public") {
        bail!("raw Sprite edge is not public; expected url auth `public`");
    }
    let origin = normalize_proxy_origin(
        info.url
            .as_deref()
            .ok_or_else(|| anyhow!("Sprite URL is unavailable"))?,
    )?;
    let raw_health = probe_http_json(&format!("{origin}/health"), "raw Sprite /health")?;
    let raw_health = parse_sprite_runtime_health(&raw_health)?;
    if raw_health.version != local_health.version {
        bail!("raw Sprite /health version does not match localhost runtime version");
    }
    println!("health-raw-sprite: ok ({origin})");

    let proxy = record.proxy.as_ref().ok_or_else(|| {
        anyhow!("Worker layer is unregistered; run `zodex sprite proxy deploy`")
    })?;
    let status = proxy_worker_status(&proxy.worker_url).context("Worker layer failed")?;
    validate_sprite_connect_worker(&status, &proxy_worker_build_id(), &origin)?;
    println!("health-worker: ok ({})", proxy.worker_url);
    let key = read_remote_sprite_mcp_key(
        &resolved.name,
        resolved.org.as_deref(),
        Path::new(&record.remote_config),
    )?;
    verify_worker_mcp_contract(&proxy.worker_url, &key)
        .await
        .context("MCP contract layer failed")?;
    println!("health-mcp-contract: ok");
    println!("sprite-health: ok");
    Ok(())
}

fn probe_http_json(url: &str, label: &str) -> Result<String> {
    run_command_capture(
        "curl",
        &[
            "-fsS".to_string(),
            "--max-time".to_string(),
            "20".to_string(),
            url.to_string(),
        ],
    )
    .with_context(|| format!("{label} request failed"))
}
