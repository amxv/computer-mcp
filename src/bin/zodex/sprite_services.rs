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

fn local_sprite_health_probe_script() -> &'static str {
    r#"set -euo pipefail
for attempt in $(seq 1 20); do
  if curl -fsS http://127.0.0.1:8080/health | grep -F '"status":"ok"' >/dev/null; then
    exit 0
  fi
  if [[ "$attempt" -lt 20 ]]; then
    sleep 1
  fi
done
echo "zodexd did not become healthy within 20 seconds" >&2
exit 1
"#
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

fn verify_local_sprite_health(sprite: &str, org: Option<&str>) -> Result<()> {
    let exec_args = vec![
        "sudo".to_string(),
        "bash".to_string(),
        "-lc".to_string(),
        local_sprite_health_probe_script().to_string(),
    ];
    run_sprite_exec(sprite, org, &exec_args, &[])?;
    Ok(())
}

fn verify_installed_sprite_release(
    sprite: &str,
    org: Option<&str>,
    requested_version: &str,
) -> Result<()> {
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
    Ok(())
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
