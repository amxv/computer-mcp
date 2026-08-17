const PROXY_WORKER_COMPONENT: &str = "zodex-cloudflare-worker";
const PROXY_WORKER_SOURCE: &str =
    include_str!("../../../proxy/cloudflare-worker/src/index.js");
const PROXY_WRANGLER_TEMPLATE: &str =
    include_str!("../../../proxy/cloudflare-worker/wrangler.jsonc");
const PROXY_PACKAGE_JSON: &str = include_str!("../../../proxy/cloudflare-worker/package.json");
const PROXY_SPRITE_ORIGIN_PLACEHOLDER: &str = "__SPRITE_ORIGIN__";
const PROXY_WORKER_NAME_PLACEHOLDER: &str = "__WORKER_NAME__";
const PROXY_WORKER_BUILD_PLACEHOLDER: &str = "__ZODEX_WORKER_BUILD__";
const MAX_WORKERS_DEV_NAME_LEN: usize = 63;
const MIN_TEMPORARY_WRANGLER_VERSION: (u64, u64, u64) = (4, 102, 0);

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpriteUrlInfo {
    url: Option<String>,
    auth: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyOriginResolution {
    origin: String,
    sprite_url_auth: Option<String>,
    sprite: Option<ResolvedSprite>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyDeployCommandSpec {
    program: String,
    base_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyOriginCheck {
    origin: String,
    sprite_url_auth: Option<String>,
    health_status: u16,
    mcp_status: u16,
    mcp_slash_status: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WranglerDeployMetadata {
    worker_name: String,
    version_id: String,
    targets: Vec<String>,
    wrangler_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyDeployResult {
    metadata: WranglerDeployMetadata,
    human_output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyWorkerStatus {
    component: String,
    build: String,
    sprite_origin: Option<String>,
}

fn validate_sprite_url_auth(url_auth: &str) -> Result<()> {
    if matches!(url_auth, "sprite" | "public") {
        Ok(())
    } else {
        bail!("url auth must be `sprite` or `public`");
    }
}

fn build_sprite_scope_args(sprite: &str, org: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(org) = org {
        args.push("-o".to_string());
        args.push(org.to_string());
    }
    args.push("-s".to_string());
    args.push(sprite.to_string());
    args
}

fn run_sprite_exec(
    sprite: &str,
    org: Option<&str>,
    exec_args: &[String],
    uploads: &[(&Path, &str)],
) -> Result<String> {
    let mut args = build_sprite_scope_args(sprite, org);
    args.push("exec".to_string());
    for (local, remote) in uploads {
        args.push("--file".to_string());
        args.push(format!("{}:{remote}", local.display()));
    }
    args.push("--".to_string());
    args.extend(exec_args.iter().cloned());
    run_command_capture("sprite", &args)
}

fn sprite_info_args(sprite: &str, org: Option<&str>) -> Vec<String> {
    let mut args = vec!["info".to_string(), "--sprite".to_string(), sprite.to_string()];
    if let Some(org) = org {
        args.extend(["--org".to_string(), org.to_string()]);
    }
    args
}

fn sprite_config_url_auth_args(sprite: &str, org: Option<&str>, url_auth: &str) -> Vec<String> {
    let mut args = vec![
        "config".to_string(),
        "update".to_string(),
        "--sprite".to_string(),
        sprite.to_string(),
        "--url-auth".to_string(),
        url_auth.to_string(),
    ];
    if let Some(org) = org {
        args.extend(["--org".to_string(), org.to_string()]);
    }
    args
}

fn parse_sprite_info(raw: &str) -> SpriteUrlInfo {
    let mut info = SpriteUrlInfo {
        url: None,
        auth: None,
    };
    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase().replace(['-', '_'], " ");
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "url" => info.url = Some(value.to_string()),
            "auth" | "url auth" | "url authentication" => info.auth = Some(value.to_string()),
            _ => {}
        }
    }
    info
}

fn sprite_url_info(sprite: &str, org: Option<&str>) -> Result<SpriteUrlInfo> {
    let raw = run_command_capture("sprite", &sprite_info_args(sprite, org))?;
    Ok(parse_sprite_info(&raw))
}

fn set_sprite_url_auth(sprite: &str, org: Option<&str>, url_auth: &str) -> Result<()> {
    validate_sprite_url_auth(url_auth)?;
    run_command_capture(
        "sprite",
        &sprite_config_url_auth_args(sprite, org, url_auth),
    )?;
    Ok(())
}

fn normalize_proxy_origin(origin: &str) -> Result<String> {
    let parsed = Url::parse(origin)
        .with_context(|| format!("failed to parse proxy origin URL `{origin}`"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        bail!("proxy origin must use http or https");
    }
    if parsed.host_str().is_none() {
        bail!("proxy origin must include a host");
    }
    if parsed.path() != "/" && !parsed.path().is_empty() {
        bail!("proxy origin must not include a path; pass the Sprite base URL only");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        bail!("proxy origin must not include a query string or fragment");
    }

    let mut normalized = parsed;
    normalized.set_path("");
    Ok(normalized.to_string().trim_end_matches('/').to_string())
}

fn resolve_proxy_origin(
    sprite: Option<&str>,
    org: Option<&str>,
    origin: Option<&str>,
) -> Result<ProxyOriginResolution> {
    if let Some(origin) = origin {
        return Ok(ProxyOriginResolution {
            origin: normalize_proxy_origin(origin)?,
            sprite_url_auth: None,
            sprite: None,
        });
    }

    let resolved = resolve_remote_sprite(sprite, org)?;
    let info = sprite_url_info(&resolved.name, resolved.org.as_deref())?;
    let url = info
        .url
        .ok_or_else(|| anyhow!("sprite URL is not available for {}", resolved.name))?;
    Ok(ProxyOriginResolution {
        origin: normalize_proxy_origin(&url)?,
        sprite_url_auth: info.auth,
        sprite: Some(resolved),
    })
}

fn proxy_worker_build_id() -> String {
    let artifact = format!(
        "{PROXY_WORKER_SOURCE}\0{PROXY_WRANGLER_TEMPLATE}\0{PROXY_PACKAGE_JSON}"
    );
    let digest = zodex::local::sha256_hex(artifact.as_bytes());
    format!("{}-{}", env!("CARGO_PKG_VERSION"), &digest[..12])
}

fn worker_identity(resolution: &ProxyOriginResolution) -> String {
    match resolution.sprite.as_ref() {
        Some(sprite) => match sprite.org.as_deref() {
            Some(org) => format!("{org}-{}", sprite.name),
            None => sprite.name.clone(),
        },
        None => Url::parse(&resolution.origin)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
            .unwrap_or_else(|| resolution.origin.clone()),
    }
}

fn sanitize_worker_name_fragment(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            normalized.push(lower);
            previous_dash = false;
        } else if !previous_dash && !normalized.is_empty() {
            normalized.push('-');
            previous_dash = true;
        }
    }
    normalized.trim_matches('-').to_string()
}

fn derive_proxy_worker_name(resolution: &ProxyOriginResolution) -> String {
    let identity = worker_identity(resolution);
    let digest = zodex::local::sha256_hex(identity.as_bytes());
    let suffix = &digest[..10];
    let mut base = sanitize_worker_name_fragment(&format!("zodex-{identity}"));
    if base.is_empty() {
        base = "zodex-sprite".to_string();
    }
    let max_base = MAX_WORKERS_DEV_NAME_LEN - suffix.len() - 1;
    if base.len() > max_base {
        base.truncate(max_base);
        base = base.trim_end_matches('-').to_string();
    }
    format!("{base}-{suffix}")
}

fn validate_worker_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_WORKERS_DEV_NAME_LEN {
        bail!("Worker name must be 1..={MAX_WORKERS_DEV_NAME_LEN} characters");
    }
    if name.starts_with('-') || name.ends_with('-') {
        bail!("Worker name must not start or end with a dash");
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        bail!("Worker name may contain only letters, numbers, and dashes");
    }
    Ok(())
}

fn render_proxy_wrangler_config(
    origin: &str,
    worker_name: &str,
    worker_build: &str,
) -> Result<String> {
    validate_worker_name(worker_name)?;
    for placeholder in [
        PROXY_SPRITE_ORIGIN_PLACEHOLDER,
        PROXY_WORKER_NAME_PLACEHOLDER,
        PROXY_WORKER_BUILD_PLACEHOLDER,
    ] {
        if !PROXY_WRANGLER_TEMPLATE.contains(placeholder) {
            bail!("proxy Wrangler template is missing placeholder {placeholder}");
        }
    }
    Ok(PROXY_WRANGLER_TEMPLATE
        .replace(PROXY_SPRITE_ORIGIN_PLACEHOLDER, origin)
        .replace(PROXY_WORKER_NAME_PLACEHOLDER, worker_name)
        .replace(PROXY_WORKER_BUILD_PLACEHOLDER, worker_build))
}

fn materialize_proxy_project(
    dir: &Path,
    resolution: &ProxyOriginResolution,
    worker_name: &str,
    worker_build: &str,
) -> Result<PathBuf> {
    let source_dir = dir.join("src");
    fs::create_dir_all(&source_dir)
        .with_context(|| format!("failed to create {}", source_dir.display()))?;
    fs::write(source_dir.join("index.js"), PROXY_WORKER_SOURCE)
        .context("failed to materialize embedded Worker source")?;
    fs::write(dir.join("package.json"), PROXY_PACKAGE_JSON)
        .context("failed to materialize embedded Worker package metadata")?;
    let config = render_proxy_wrangler_config(&resolution.origin, worker_name, worker_build)?;
    let config_path = dir.join("wrangler.jsonc");
    fs::write(&config_path, config)
        .context("failed to materialize embedded Wrangler configuration")?;
    Ok(config_path)
}

fn resolve_proxy_deploy_command() -> Result<ProxyDeployCommandSpec> {
    if command_exists("wrangler") {
        return Ok(ProxyDeployCommandSpec {
            program: "wrangler".to_string(),
            base_args: Vec::new(),
        });
    }
    if command_exists("bunx") {
        return Ok(ProxyDeployCommandSpec {
            program: "bunx".to_string(),
            base_args: vec!["wrangler".to_string()],
        });
    }
    if command_exists("npx") {
        return Ok(ProxyDeployCommandSpec {
            program: "npx".to_string(),
            base_args: vec!["--yes".to_string(), "wrangler".to_string()],
        });
    }
    bail!(
        "Cloudflare Wrangler is unavailable. Install Node.js plus Wrangler, or make `wrangler`, `bunx`, or `npx` available on PATH."
    )
}

fn parse_wrangler_version(raw: &str) -> Option<(u64, u64, u64)> {
    raw.split_whitespace().find_map(|token| {
        let token = token.trim_start_matches('v');
        let mut parts = token.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch_digits = parts
            .next()?
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if patch_digits.is_empty() {
            return None;
        }
        let patch = patch_digits.parse().ok()?;
        Some((major, minor, patch))
    })
}

#[allow(dead_code)] // Used by the first-run temporary deployment path added in Phase 5.
fn ensure_temporary_wrangler_version(raw: &str) -> Result<(u64, u64, u64)> {
    let version = parse_wrangler_version(raw)
        .ok_or_else(|| anyhow!("failed to parse Wrangler version from `{}`", raw.trim()))?;
    if version < MIN_TEMPORARY_WRANGLER_VERSION {
        bail!(
            "temporary Cloudflare deployment requires Wrangler >=4.102.0; found {}.{}.{}",
            version.0,
            version.1,
            version.2
        );
    }
    Ok(version)
}

#[allow(dead_code)] // Used by the first-run temporary deployment path added in Phase 5.
fn proxy_deploy_runner_version(deploy: &ProxyDeployCommandSpec) -> Result<String> {
    let mut args = deploy.base_args.clone();
    args.push("--version".to_string());
    run_command_capture(&deploy.program, &args)
}

fn parse_wrangler_deploy_output(raw: &str) -> Result<WranglerDeployMetadata> {
    let mut session_version = None;
    let mut deploy = None;
    let mut failure = None;

    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line).with_context(|| {
            format!("invalid Wrangler structured output on line {}", index + 1)
        })?;
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("wrangler-session") => {
                session_version = value
                    .get("wrangler_version")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
            }
            Some("deploy") => {
                let worker_name = value
                    .get("worker_name")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| anyhow!("Wrangler deploy record is missing worker_name"))?;
                let version_id = value
                    .get("version_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| anyhow!("Wrangler deploy record is missing version_id"))?;
                let targets = value
                    .get("targets")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| anyhow!("Wrangler deploy record is missing targets"))?
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if targets.is_empty() {
                    bail!("Wrangler deploy record did not contain a deployment target");
                }
                deploy = Some((worker_name.to_string(), version_id.to_string(), targets));
            }
            Some("command-failed") => {
                failure = value
                    .get("message")
                    .or_else(|| value.get("error").and_then(|error| error.get("message")))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
            }
            _ => {}
        }
    }

    let (worker_name, version_id, targets) = deploy.ok_or_else(|| {
        if let Some(failure) = failure {
            anyhow!("Wrangler deployment failed: {failure}")
        } else {
            anyhow!("Wrangler structured output did not contain a deploy record")
        }
    })?;
    Ok(WranglerDeployMetadata {
        worker_name,
        version_id,
        targets,
        wrangler_version: session_version,
    })
}

fn execute_wrangler_deploy(
    deploy: &ProxyDeployCommandSpec,
    project_dir: &Path,
    config_path: &Path,
    cloudflare_account: Option<&str>,
) -> Result<ProxyDeployResult> {
    let structured_path = project_dir.join("wrangler-output.ndjson");
    let mut args = deploy.base_args.clone();
    args.extend([
        "deploy".to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
    ]);
    let mut command = Command::new(&deploy.program);
    command
        .args(&args)
        .current_dir(project_dir)
        .env("WRANGLER_OUTPUT_FILE_PATH", &structured_path)
        .env("FORCE_COLOR", "0");
    if let Some(account) = cloudflare_account {
        command.env("CLOUDFLARE_ACCOUNT_ID", account);
    }
    let output = command
        .output()
        .with_context(|| format!("failed to run {}", deploy.program))?;
    let stdout = redact_api_key_query_params(&String::from_utf8_lossy(&output.stdout));
    let stderr = redact_api_key_query_params(&String::from_utf8_lossy(&output.stderr));
    let structured = fs::read_to_string(&structured_path).ok();

    if !output.status.success() {
        let structured_error = structured
            .as_deref()
            .and_then(|raw| parse_wrangler_deploy_output(raw).err())
            .map(|error| error.to_string())
            .unwrap_or_else(|| "Wrangler deployment failed".to_string());
        let details = [stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if details.is_empty() {
            bail!("{structured_error}");
        }
        bail!("{structured_error}\n{details}");
    }

    let structured = structured.ok_or_else(|| {
        anyhow!(
            "Wrangler did not write structured deployment output to {}",
            structured_path.display()
        )
    })?;
    Ok(ProxyDeployResult {
        metadata: parse_wrangler_deploy_output(&structured)?,
        human_output: stdout,
    })
}

fn proxy_worker_build_state(status: &ProxyWorkerStatus, expected_build: &str) -> &'static str {
    if status.component != PROXY_WORKER_COMPONENT {
        "foreign"
    } else if status.build == expected_build {
        "current"
    } else {
        "stale"
    }
}

fn proxy_worker_status(url: &str) -> Result<ProxyWorkerStatus> {
    let status_url = format!("{}/status", url.trim_end_matches('/'));
    let raw = run_command_capture(
        "curl",
        &[
            "-fsS".to_string(),
            "--max-time".to_string(),
            "20".to_string(),
            status_url.clone(),
        ],
    )?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse Worker status from {status_url}"))?;
    Ok(ProxyWorkerStatus {
        component: value
            .get("component")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        build: value
            .get("build")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        sprite_origin: value
            .get("spriteOrigin")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

fn inspect_proxy_component(
    sprite: Option<&str>,
    org: Option<&str>,
    origin: Option<&str>,
    worker_name: Option<&str>,
    worker_url: Option<&str>,
) -> Result<()> {
    let resolution = resolve_proxy_origin(sprite, org, origin)?;
    let derived_name = worker_name
        .map(str::to_string)
        .unwrap_or_else(|| derive_proxy_worker_name(&resolution));
    validate_worker_name(&derived_name)?;
    let build = proxy_worker_build_id();
    println!("component: {PROXY_WORKER_COMPONENT}");
    println!("worker-name: {derived_name}");
    println!("embedded-build: {build}");
    println!("sprite-origin: {}", resolution.origin);
    if let Some(auth) = resolution.sprite_url_auth.as_deref() {
        println!("sprite-url-auth: {auth}");
    }
    println!("routes: /, /status, /health, /mcp, /mcp/");
    println!("mcp-dispatch: at-most-once-after-readiness");
    if let Some(worker_url) = worker_url {
        let status = proxy_worker_status(worker_url)?;
        println!("deployed-component: {}", status.component);
        println!("deployed-build: {}", status.build);
        println!("build-state: {}", proxy_worker_build_state(&status, &build));
    }
    match resolve_proxy_deploy_command() {
        Ok(command) => println!(
            "deploy-runner: {} {}",
            command.program,
            command.base_args.join(" ")
        ),
        Err(error) => println!("deploy-runner: unavailable ({error})"),
    }
    Ok(())
}

fn deploy_proxy_component(
    sprite: Option<&str>,
    org: Option<&str>,
    origin: Option<&str>,
    worker_name: Option<&str>,
    cloudflare_account: Option<&str>,
    skip_verify_origin: bool,
) -> Result<()> {
    let resolution = resolve_proxy_origin(sprite, org, origin)?;
    ensure_proxy_origin_is_publicly_routable(&resolution)?;
    if !skip_verify_origin {
        print_proxy_origin_check(&verify_proxy_origin(&resolution)?);
    }
    let name = worker_name
        .map(str::to_string)
        .unwrap_or_else(|| derive_proxy_worker_name(&resolution));
    validate_worker_name(&name)?;
    let build = proxy_worker_build_id();
    let project = tempfile::tempdir().context("failed to create temporary Worker project")?;
    let config_path = materialize_proxy_project(project.path(), &resolution, &name, &build)?;
    let deploy = resolve_proxy_deploy_command()?;
    let result = execute_wrangler_deploy(
        &deploy,
        project.path(),
        &config_path,
        cloudflare_account,
    )?;
    if !result.human_output.trim().is_empty() {
        print!("{}", result.human_output);
    }
    println!("proxy-origin: {}", resolution.origin);
    println!("worker-name: {}", result.metadata.worker_name);
    println!("worker-version: {}", result.metadata.version_id);
    println!("worker-build: {build}");
    if let Some(version) = result.metadata.wrangler_version.as_deref() {
        println!("wrangler-version: {version}");
    }
    for target in &result.metadata.targets {
        println!("worker-target: {target}");
    }
    println!("proxy-deploy: complete");
    Ok(())
}

fn verify_proxy_command(
    sprite: Option<&str>,
    org: Option<&str>,
    origin: Option<&str>,
    worker_url: Option<&str>,
) -> Result<()> {
    let resolution = resolve_proxy_origin(sprite, org, origin)?;
    print_proxy_origin_check(&verify_proxy_origin(&resolution)?);
    if let Some(worker_url) = worker_url {
        let status = proxy_worker_status(worker_url)?;
        if status.component != PROXY_WORKER_COMPONENT {
            bail!("Worker status reported unexpected component `{}`", status.component);
        }
        let expected_build = proxy_worker_build_id();
        if status.build != expected_build {
            bail!(
                "Worker build is stale: deployed `{}`, embedded `{expected_build}`",
                status.build
            );
        }
        if status.sprite_origin.as_deref() != Some(resolution.origin.as_str()) {
            bail!("Worker status origin does not match the selected Sprite origin");
        }
        let worker_base = worker_url.trim_end_matches('/');
        let health_status = probe_http_status(&format!("{worker_base}/health"))?;
        let mcp_status = probe_http_status(&format!("{worker_base}/mcp"))?;
        if health_status != 200 || !proxy_mcp_status_looks_healthy(mcp_status) {
            bail!(
                "Worker front door probe failed: health HTTP {health_status}, MCP HTTP {mcp_status}"
            );
        }
        println!("worker-url: {worker_base}");
        println!("worker-build: {}", status.build);
        println!("worker-health-status: {health_status}");
        println!("worker-mcp-status: {mcp_status}");
        println!("proxy-worker-check: ok");
    }
    Ok(())
}

fn ensure_proxy_origin_is_publicly_routable(resolution: &ProxyOriginResolution) -> Result<()> {
    if let Some(auth) = resolution.sprite_url_auth.as_deref()
        && auth != "public"
    {
        bail!(
            "sprite URL auth is `{auth}` for {}. The canonical Worker requires a public Sprite origin. Run `sprite config update --url-auth public --sprite <name>` before deploying it.",
            resolution.origin
        );
    }
    Ok(())
}

fn verify_proxy_origin(resolution: &ProxyOriginResolution) -> Result<ProxyOriginCheck> {
    let base = resolution.origin.trim_end_matches('/');
    let health_status = probe_http_status(&format!("{base}/health"))?;
    let mcp_status = probe_http_status(&format!("{base}/mcp"))?;
    let mcp_slash_status = probe_http_status(&format!("{base}/mcp/"))?;
    if health_status != 200 {
        bail!("raw Sprite origin health probe returned HTTP {health_status} for {base}/health");
    }
    if !proxy_mcp_status_looks_healthy(mcp_status) {
        bail!("raw Sprite origin `/mcp` probe returned HTTP {mcp_status}; expected 200 or 401");
    }
    if !proxy_mcp_status_looks_healthy(mcp_slash_status) {
        bail!(
            "raw Sprite origin `/mcp/` probe returned HTTP {mcp_slash_status}; expected 200 or 401"
        );
    }
    Ok(ProxyOriginCheck {
        origin: resolution.origin.clone(),
        sprite_url_auth: resolution.sprite_url_auth.clone(),
        health_status,
        mcp_status,
        mcp_slash_status,
    })
}

fn print_proxy_origin_check(check: &ProxyOriginCheck) {
    println!("origin: {}", check.origin);
    if let Some(auth) = check.sprite_url_auth.as_deref() {
        println!("sprite-url-auth: {auth}");
    }
    println!("health-status: {}", check.health_status);
    println!("mcp-status: {}", check.mcp_status);
    println!("mcp-slash-status: {}", check.mcp_slash_status);
    println!("proxy-origin-check: ok");
}

fn proxy_mcp_status_looks_healthy(status: u16) -> bool {
    matches!(status, 200 | 401)
}

fn probe_http_status(url: &str) -> Result<u16> {
    let raw = run_command_capture(
        "curl",
        &[
            "-sS".to_string(),
            "-o".to_string(),
            "/dev/null".to_string(),
            "-w".to_string(),
            "%{http_code}".to_string(),
            "--max-time".to_string(),
            "20".to_string(),
            url.to_string(),
        ],
    )?;
    raw.trim()
        .parse::<u16>()
        .with_context(|| format!("failed to parse HTTP status from curl probe for {url}: {raw}"))
}
