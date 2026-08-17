fn build_sprite_detached_stop_script(config_path: &Path) -> String {
    let daemon_command = format!(
        "/usr/local/bin/{SPRITE_MAIN_SERVICE_LABEL} --config {}",
        config_path.display()
    );
    let publisher_command = format!(
        "/usr/local/bin/{PUBLISHER_SERVICE_LABEL} --config {}",
        config_path.display()
    );
    format!(
        "pkill -f -x -- {} || true; pkill -f -x -- {} || true",
        shell_escape_single_quotes(&daemon_command),
        shell_escape_single_quotes(&publisher_command)
    )
}

fn sprite_service_delete_order() -> [&'static str; 2] {
    // Delete dependents before dependencies. zodexd declares `needs: zodex-prd`.
    [SPRITE_MAIN_SERVICE_LABEL, PUBLISHER_SERVICE_LABEL]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteServiceAction {
    Start,
    Stop,
    Restart,
}

impl SpriteServiceAction {
    fn endpoint_suffix(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

fn sprite_service_post_args() -> Vec<String> {
    vec!["-sS".to_string(), "-X".to_string(), "POST".to_string()]
}

fn parse_sprite_service_event_values(raw: &str) -> Result<Vec<serde_json::Value>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("Sprite Service operation returned an empty event stream");
    }

    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed)
            .context("failed to parse Sprite Service event stream JSON array");
    }

    trimmed
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).with_context(|| {
                format!("failed to parse Sprite Service NDJSON event at line {}", index + 1)
            })
        })
        .collect()
}

fn validate_sprite_service_operation_stream(
    service_name: &str,
    action: SpriteServiceAction,
    raw: &str,
) -> Result<()> {
    if action == SpriteServiceAction::Stop && raw.trim() == "service is not running" {
        return Ok(());
    }

    let events = parse_sprite_service_event_values(raw)?;
    let mut terminal_type = None;

    for event in &events {
        let event_type = event
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("Sprite Service event is missing a string `type` field"))?;
        terminal_type = Some(event_type);

        if event_type == "error" {
            let detail = event
                .get("data")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("provider returned an unspecified service error");
            bail!(
                "Sprite Service {service_name} {} failed: {detail}",
                action.endpoint_suffix()
            );
        }
    }

    if terminal_type != Some("complete") {
        bail!(
            "Sprite Service {service_name} {} did not end with a terminal `complete` event",
            action.endpoint_suffix()
        );
    }

    Ok(())
}

fn parse_sprite_services_response(raw: &str) -> Result<Vec<SpriteServiceStatus>> {
    serde_json::from_str(raw).context("failed to parse Sprite Services response")
}

fn ensure_sprite_services_running(
    services: &[SpriteServiceStatus],
    expected_running: &[&str],
) -> Result<()> {
    for service_name in expected_running {
        let service = services
            .iter()
            .find(|service| service.name == *service_name)
            .ok_or_else(|| anyhow!("Sprite Service {service_name} is missing after restart"))?;
        let status = service
            .state
            .as_ref()
            .and_then(|state| state.status.as_deref())
            .unwrap_or("unknown");
        if status != "running" {
            bail!("Sprite Service {service_name} is {status} after restart, expected running");
        }
    }
    Ok(())
}

fn run_sprite_service_action_with_api<F>(
    api: &mut F,
    service_name: &str,
    action: SpriteServiceAction,
) -> Result<()>
where
    F: FnMut(&str, &[String]) -> Result<String>,
{
    let path = format!(
        "/services/{service_name}/{}",
        action.endpoint_suffix()
    );
    let raw = api(&path, &sprite_service_post_args())?;
    validate_sprite_service_operation_stream(service_name, action, &raw)
}

fn restart_sprite_service_stack_with<F>(mut api: F) -> Result<()>
where
    F: FnMut(&str, &[String]) -> Result<String>,
{
    // Stop the dependent admission path before disrupting the publisher dependency.
    run_sprite_service_action_with_api(
        &mut api,
        SPRITE_MAIN_SERVICE_LABEL,
        SpriteServiceAction::Stop,
    )?;

    run_sprite_service_action_with_api(
        &mut api,
        PUBLISHER_SERVICE_LABEL,
        SpriteServiceAction::Restart,
    )?;
    let services = parse_sprite_services_response(&api(
        "/services",
        &["-sS".to_string()],
    )?)?;
    ensure_sprite_services_running(&services, &[PUBLISHER_SERVICE_LABEL])?;

    run_sprite_service_action_with_api(
        &mut api,
        SPRITE_MAIN_SERVICE_LABEL,
        SpriteServiceAction::Start,
    )?;
    let services = parse_sprite_services_response(&api(
        "/services",
        &["-sS".to_string()],
    )?)?;
    ensure_sprite_services_running(
        &services,
        &[PUBLISHER_SERVICE_LABEL, SPRITE_MAIN_SERVICE_LABEL],
    )?;
    Ok(())
}

fn restart_sprite_services(sprite: &str, org: Option<&str>) -> Result<()> {
    restart_sprite_service_stack_with(|path, args| run_sprite_api(sprite, org, path, args))?;
    println!("sprite services restarted for {sprite}");
    Ok(())
}

fn sync_sprite_services(
    sprite: &str,
    org: Option<&str>,
    config_path: &Path,
    force_recreate: bool,
    skip_stop_detached: bool,
) -> Result<()> {
    if !skip_stop_detached {
        let stop_args = vec![
            "sudo".to_string(),
            "bash".to_string(),
            "-lc".to_string(),
            build_sprite_detached_stop_script(config_path),
        ];
        if let Err(err) = run_sprite_exec(sprite, org, &stop_args, &[]) {
            eprintln!("warning: failed to stop detached daemons before Sprite sync: {err}");
        }
    }

    if force_recreate {
        for service_name in sprite_service_delete_order() {
            let status = run_sprite_api(
                sprite,
                org,
                &format!("/services/{service_name}"),
                &[
                    "-sS".to_string(),
                    "-o".to_string(),
                    "/dev/null".to_string(),
                    "-w".to_string(),
                    "%{http_code}\n".to_string(),
                    "-X".to_string(),
                    "DELETE".to_string(),
                ],
            )?;
            let trimmed = status.trim();
            if trimmed != "204" && trimmed != "404" {
                bail!("failed to delete Sprite service {service_name} (HTTP {trimmed})");
            }
        }
    }

    for (service_name, definition) in expected_sprite_service_definitions(config_path) {
        let payload = serde_json::to_string(&definition).context("failed to encode service")?;
        run_sprite_api(
            sprite,
            org,
            &format!("/services/{service_name}"),
            &[
                "-sS".to_string(),
                "-X".to_string(),
                "PUT".to_string(),
                "-H".to_string(),
                "Content-Type: application/json".to_string(),
                "-d".to_string(),
                payload,
            ],
        )?;
    }

    println!("sprite services synced for {sprite}");
    Ok(())
}

fn verify_sprite_service_logs(sprite: &str, org: Option<&str>) -> Result<()> {
    for service in [PUBLISHER_SERVICE_LABEL, SPRITE_MAIN_SERVICE_LABEL] {
        let path = sprite_service_logs_api_path(service, Some(20), None);
        run_sprite_api(sprite, org, &path, &["-sS".to_string()])?;
    }
    Ok(())
}

fn verify_installed_sprite_release(
    sprite: &str,
    org: Option<&str>,
    requested_version: &str,
) -> Result<String> {
    let exec_args = vec![
        "sudo".to_string(),
        "-u".to_string(),
        ZODEX_AGENT_USER.to_string(),
        "env".to_string(),
        format!("HOME={ZODEX_AGENT_HOME}"),
        ZODEX_AGENT_BINARY_PATH.to_string(),
        "--version".to_string(),
    ];
    let output = run_sprite_exec(sprite, org, &exec_args, &[])?;
    let installed_version = validate_installed_sprite_release(&output, requested_version)?;
    println!("sprite-runtime-version: {installed_version}");
    Ok(installed_version)
}

fn validate_installed_sprite_release(output: &str, requested_version: &str) -> Result<String> {
    let installed_version = output
        .split_whitespace()
        .last()
        .ok_or_else(|| anyhow!("installed zodex-agent did not report a version"))?
        .to_string();

    if requested_version != "latest" {
        let expected_version = requested_version.trim_start_matches('v');
        if installed_version != expected_version {
            bail!(
                "Sprite installed zodex-agent {installed_version}, expected {expected_version}"
            );
        }
    }
    Ok(installed_version)
}
