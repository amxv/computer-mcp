fn print_sprite_services_status_summary(
    config_path: &Path,
    sprite: &str,
    org: Option<&str>,
) -> Result<()> {
    let services = fetch_sprite_services(sprite, org)?;
    let lines = build_sprite_services_status_lines(config_path, sprite, &services);
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

fn print_sprite_service_logs(
    sprite: &str,
    org: Option<&str>,
    service: &str,
    lines: Option<usize>,
    duration: Option<&str>,
) -> Result<()> {
    let path = sprite_service_logs_api_path(service, lines, duration);
    let raw = run_sprite_api(sprite, org, &path, &["-sS".to_string()])?;

    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(parsed) => println!(
            "{}",
            serde_json::to_string_pretty(&parsed)
                .context("failed to format Sprite Service logs")?
        ),
        Err(_) => print!("{raw}"),
    }

    Ok(())
}

fn fetch_sprite_services(sprite: &str, org: Option<&str>) -> Result<Vec<SpriteServiceStatus>> {
    let raw = run_sprite_api(sprite, org, "/services", &["-sS".to_string()])?;
    serde_json::from_str(&raw).context("failed to parse Sprite Services response")
}

fn build_sprite_services_status_lines(
    config_path: &Path,
    sprite: &str,
    services: &[SpriteServiceStatus],
) -> Vec<String> {
    let expected = expected_sprite_service_definitions(config_path);
    let service_map: BTreeMap<&str, &SpriteServiceStatus> = services
        .iter()
        .map(|service| (service.name.as_str(), service))
        .collect();

    let mut lines = vec![
        "service-mode: sprite-services".to_string(),
        format!("sprite: {sprite}"),
        format!("config: {}", config_path.display()),
        format!("source-of-truth: sprite api -s {sprite} /services"),
    ];

    for service_name in [PUBLISHER_SERVICE_LABEL, SPRITE_MAIN_SERVICE_LABEL] {
        lines.push(String::new());
        lines.extend(build_single_sprite_service_status_lines(
            service_name,
            sprite,
            service_map.get(service_name).copied(),
            expected.get(service_name),
        ));
    }

    lines
}

fn build_single_sprite_service_status_lines(
    service_name: &str,
    sprite: &str,
    actual: Option<&SpriteServiceStatus>,
    expected: Option<&SpriteServiceDefinition>,
) -> Vec<String> {
    let mut lines = vec![format!("service: {service_name}")];

    let expected_run_user = if service_name == PUBLISHER_SERVICE_LABEL {
        ZODEX_PUBLISHER_USER
    } else {
        ZODEX_AGENT_USER
    };
    lines.push(format!("expected-run-user: {expected_run_user}"));

    let Some(service) = actual else {
        lines.push("active: missing".to_string());
        lines.push(format!(
            "hint: register Sprite Services with `zodex sprite sync --sprite {sprite}`"
        ));
        return lines;
    };

    let status = service
        .state
        .as_ref()
        .and_then(|state| state.status.as_deref())
        .unwrap_or("unknown");
    let pid = service
        .state
        .as_ref()
        .and_then(|state| state.pid)
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let started_at = service
        .state
        .as_ref()
        .and_then(|state| state.started_at.as_deref())
        .unwrap_or("unknown");

    lines.push(format!("active: {status}"));
    lines.push(format!("pid: {pid}"));
    lines.push(format!("started-at: {started_at}"));
    lines.push(format!(
        "http-port: {}",
        service
            .http_port
            .map(|port| port.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    ));
    lines.push(format!(
        "needs: {}",
        if service.needs.is_empty() {
            "none".to_string()
        } else {
            service.needs.join(", ")
        }
    ));
    lines.push(format!("cmd: {}", service.cmd));
    lines.push(format!("args: {}", service.args.join(" ")));

    if let Some(expected_definition) = expected {
        let matches = sprite_service_matches_definition(service, expected_definition);
        lines.push(format!(
            "definition-match: {}",
            if matches { "yes" } else { "no" }
        ));
        if !matches {
            lines.push(format!(
                "hint: re-sync with `zodex sprite sync --sprite {sprite}`"
            ));
        }
    }

    if status != "running" {
        lines.push(format!(
            "hint: inspect logs with `{PRIMARY_OPERATOR_BINARY} sprite logs --sprite {sprite} --service {service_name}`"
        ));
    }

    lines
}

fn expected_sprite_service_definitions(
    config_path: &Path,
) -> BTreeMap<&'static str, SpriteServiceDefinition> {
    let config_arg = config_path.display().to_string();
    BTreeMap::from([
        (
            PUBLISHER_SERVICE_LABEL,
            SpriteServiceDefinition {
                cmd: "sudo".to_string(),
                args: vec![
                    "-n".to_string(),
                    "-u".to_string(),
                    ZODEX_PUBLISHER_USER.to_string(),
                    format!("/usr/local/bin/{PUBLISHER_SERVICE_LABEL}"),
                    "--config".to_string(),
                    config_arg.clone(),
                ],
                needs: Vec::new(),
                http_port: None,
            },
        ),
        (
            SPRITE_MAIN_SERVICE_LABEL,
            SpriteServiceDefinition {
                cmd: "sudo".to_string(),
                args: vec![
                    "-n".to_string(),
                    "-u".to_string(),
                    ZODEX_AGENT_USER.to_string(),
                    format!("/usr/local/bin/{SPRITE_MAIN_SERVICE_LABEL}"),
                    "--config".to_string(),
                    config_arg,
                ],
                needs: vec![PUBLISHER_SERVICE_LABEL.to_string()],
                http_port: Some(8080),
            },
        ),
    ])
}

fn sprite_service_matches_definition(
    actual: &SpriteServiceStatus,
    expected: &SpriteServiceDefinition,
) -> bool {
    actual.cmd == expected.cmd
        && actual.args == expected.args
        && actual.needs == expected.needs
        && actual.http_port == expected.http_port
}

fn sprite_service_logs_api_path(
    service: &str,
    lines: Option<usize>,
    duration: Option<&str>,
) -> String {
    let mut query = Vec::new();
    if let Some(lines) = lines {
        query.push(format!("lines={lines}"));
    }
    if let Some(duration) = duration
        && !duration.is_empty()
    {
        query.push(format!("duration={duration}"));
    }

    if query.is_empty() {
        format!("/services/{service}/logs")
    } else {
        format!("/services/{service}/logs?{}", query.join("&"))
    }
}

fn run_sprite_api(
    sprite: &str,
    org: Option<&str>,
    path: &str,
    curl_args: &[String],
) -> Result<String> {
    if !command_exists("sprite") {
        bail!("sprite CLI is required for Sprite service inspection");
    }

    let raw = run_command_capture(
        "sprite",
        &build_sprite_api_args(sprite, org, path, curl_args),
    )?;
    Ok(strip_sprite_api_prelude(&raw))
}

fn build_sprite_api_args(
    sprite: &str,
    org: Option<&str>,
    path: &str,
    curl_args: &[String],
) -> Vec<String> {
    let mut args = vec!["api".to_string()];
    if let Some(org) = org {
        args.push("-o".to_string());
        args.push(org.to_string());
    }
    args.push("-s".to_string());
    args.push(sprite.to_string());
    args.push(path.to_string());
    if !curl_args.is_empty() {
        args.push("--".to_string());
        args.extend(curl_args.iter().cloned());
    }
    args
}

fn strip_sprite_api_prelude(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.len() >= 2 && lines[0].starts_with("Calling API:") && lines[1].starts_with("URL:") {
        let mut stripped = lines[2..].join("\n");
        if raw.ends_with('\n') && !stripped.ends_with('\n') {
            stripped.push('\n');
        }
        return stripped.trim_start_matches('\n').to_string();
    }

    raw.to_string()
}
