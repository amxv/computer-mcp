#[derive(Debug, Clone, PartialEq, Eq)]
struct SpriteConnectPresentation {
    capability_url: String,
    copied_to_clipboard: bool,
    print_capability_url: bool,
}

fn read_remote_sprite_mcp_key(
    sprite: &str,
    org: Option<&str>,
    remote_config: &Path,
) -> Result<String> {
    let exec_args = vec![
        "sudo".to_string(),
        "cat".to_string(),
        remote_config.display().to_string(),
    ];
    let raw = run_sprite_exec_sensitive(sprite, org, &exec_args)?;
    parse_remote_sprite_mcp_key(&raw)
}

fn parse_remote_sprite_mcp_key(raw: &str) -> Result<String> {
    let parsed: toml::Value = toml::from_str(raw).context("failed to parse remote Sprite config")?;
    let key = parsed
        .get("api_key")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("remote Sprite config is missing api_key"))?;
    Ok(key.to_string())
}

fn build_sprite_connect_url(worker_url: &str, key: &str) -> Result<String> {
    let worker_url = normalize_worker_url(worker_url)?;
    let mut url = Url::parse(&worker_url).context("failed to parse registered Worker URL")?;
    url.set_path("/mcp");
    url.set_query(None);
    url.query_pairs_mut().append_pair("key", key);
    Ok(url.to_string())
}

fn prepare_sprite_connect_presentation<F>(
    worker_url: &str,
    key: &str,
    show_url: bool,
    copy: F,
) -> Result<SpriteConnectPresentation>
where
    F: FnOnce(&str) -> bool,
{
    let capability_url = build_sprite_connect_url(worker_url, key)?;
    let copied_to_clipboard = copy(&capability_url);
    Ok(SpriteConnectPresentation {
        capability_url,
        copied_to_clipboard,
        print_capability_url: show_url || !copied_to_clipboard,
    })
}

fn validate_sprite_connect_worker(
    status: &ProxyWorkerStatus,
    expected_build: &str,
    expected_origin: &str,
) -> Result<()> {
    if proxy_worker_build_state(status, expected_build) != "current" {
        bail!("registered Worker is stale or foreign; run `zodex sprite proxy deploy`");
    }
    if status.sprite_origin.as_deref() != Some(expected_origin) {
        bail!(
            "registered Worker points at a different Sprite origin; run `zodex sprite proxy deploy`"
        );
    }
    Ok(())
}

fn connect_sprite(sprite: Option<&str>, org: Option<&str>, show_url: bool) -> Result<()> {
    let resolved = resolve_remote_sprite(sprite, org)?;
    let record = load_operator_sprite_record(&resolved)?.ok_or_else(|| {
        anyhow!(
            "Sprite `{}` is not registered locally; run `zodex sprite setup` first",
            resolved.name
        )
    })?;
    let proxy = record.proxy.ok_or_else(|| {
        anyhow!(
            "Sprite `{}` has no permanent Worker registered; run `zodex sprite proxy deploy` first",
            resolved.name
        )
    })?;
    let expected_build = proxy_worker_build_id();
    if proxy.worker_build != expected_build {
        bail!(
            "registered Worker build is stale; run `zodex sprite proxy deploy` before connecting"
        );
    }
    let status = proxy_worker_status(&proxy.worker_url).with_context(|| {
        "registered Worker is unreachable; run `zodex sprite proxy deploy` to repair it"
    })?;
    let sprite_info = sprite_url_info(&resolved.name, resolved.org.as_deref())?;
    let sprite_origin = sprite_info
        .url
        .ok_or_else(|| anyhow!("Sprite URL is unavailable; rerun `zodex sprite proxy deploy`"))?;
    let sprite_origin = normalize_proxy_origin(&sprite_origin)?;
    validate_sprite_connect_worker(&status, &expected_build, &sprite_origin)?;

    let key = read_remote_sprite_mcp_key(
        &resolved.name,
        resolved.org.as_deref(),
        Path::new(&record.remote_config),
    )?;
    let presentation = prepare_sprite_connect_presentation(
        &proxy.worker_url,
        &key,
        show_url,
        best_effort_copy_to_clipboard,
    )?;

    println!("sprite: {}", resolved.name);
    if let Some(org) = resolved.org.as_deref() {
        println!("org: {org}");
    }
    println!("worker-url: {}", proxy.worker_url);
    if presentation.copied_to_clipboard {
        println!("mcp-endpoint: copied to clipboard (contains the secret Sprite capability key)");
    } else {
        println!("mcp-endpoint: clipboard unavailable; printing because `sprite connect` was explicitly invoked");
    }
    if presentation.print_capability_url {
        println!("mcp-url: {}", presentation.capability_url);
    }
    println!("ChatGPT setup:");
    println!("  1. Enable Developer Mode for custom apps in Settings → Apps (or Workspace Settings → Apps).");
    println!("  2. Create a custom app and provide the MCP endpoint copied above.");
    println!("  3. The endpoint already contains Zodex's capability secret; do not copy it into unrelated auth fields.");
    println!("  4. Scan Tools, confirm the Zodex tools, then Create the app.");
    println!("note: full write/modify MCP requires a ChatGPT workspace/plan that currently supports full MCP.");
    Ok(())
}
