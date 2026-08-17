#[derive(Debug, Clone, PartialEq, Eq)]
struct CloudflareAccount {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WranglerWhoamiPayload {
    logged_in: bool,
    #[serde(default)]
    accounts: Vec<WranglerWhoamiAccount>,
}

#[derive(Debug, Deserialize)]
struct WranglerWhoamiAccount {
    id: String,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WranglerWhoamiState {
    Authenticated(Vec<CloudflareAccount>),
    Unauthenticated,
}

#[derive(Debug)]
struct WranglerDeployAttemptOutput {
    success: bool,
    stdout: String,
    stderr: String,
    structured: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TemporaryCloudflareDeployment {
    worker_url: String,
    claim_url: String,
    worker_name: Option<String>,
    worker_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PermanentCloudflareDeployment {
    cloudflare_account_id: String,
    metadata: WranglerDeployMetadata,
    worker_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CloudflareDeployOutcome {
    Permanent(PermanentCloudflareDeployment),
    Temporary(TemporaryCloudflareDeployment),
}

fn run_wrangler_whoami(deploy: &ProxyDeployCommandSpec) -> Result<WranglerWhoamiState> {
    let mut args = deploy.base_args.clone();
    args.extend(["whoami".to_string(), "--json".to_string()]);
    let output = Command::new(&deploy.program)
        .args(&args)
        .env("FORCE_COLOR", "0")
        .output()
        .with_context(|| format!("failed to run {} whoami", deploy.program))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        let payload: WranglerWhoamiPayload = serde_json::from_str(stdout.trim())
            .context("failed to parse `wrangler whoami --json` output")?;
        if !payload.logged_in {
            return Ok(WranglerWhoamiState::Unauthenticated);
        }
        return Ok(WranglerWhoamiState::Authenticated(
            payload
                .accounts
                .into_iter()
                .map(|account| CloudflareAccount {
                    id: account.id,
                    name: account.name,
                })
                .collect(),
        ));
    }

    let combined = format!("{stdout}\n{stderr}");
    if wrangler_whoami_is_unauthenticated(&combined) {
        return Ok(WranglerWhoamiState::Unauthenticated);
    }
    bail!(
        "failed to inspect Cloudflare account membership with `wrangler whoami --json`: {}",
        sanitize_wrangler_output(&combined).trim()
    )
}

fn wrangler_whoami_is_unauthenticated(output: &str) -> bool {
    let compact = output
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    compact.contains("\"loggedin\":false")
        || output.to_ascii_lowercase().contains("not authenticated")
}

fn select_cloudflare_account(
    state: &WranglerWhoamiState,
    requested: Option<&str>,
    registered_account_id: Option<&str>,
) -> Result<Option<CloudflareAccount>> {
    match state {
        WranglerWhoamiState::Unauthenticated => {
            if let Some(requested) = requested {
                if registered_account_id == Some(requested) {
                    return Ok(Some(CloudflareAccount {
                        id: requested.to_string(),
                        name: String::new(),
                    }));
                }
                bail!(
                    "cannot resolve Cloudflare account `{requested}` while Wrangler is unauthenticated; authenticate with `wrangler login --use-keyring` first"
                );
            }
            Ok(registered_account_id.map(|id| CloudflareAccount {
                id: id.to_string(),
                name: String::new(),
            }))
        }
        WranglerWhoamiState::Authenticated(accounts) => {
            if accounts.is_empty() {
                bail!("Wrangler is authenticated but reports no eligible Cloudflare accounts");
            }

            if let Some(requested) = requested {
                let mut matches = accounts.iter().filter(|account| {
                    account.id == requested || account.name.eq_ignore_ascii_case(requested)
                });
                let Some(first) = matches.next() else {
                    bail!(
                        "Cloudflare account `{requested}` is not present in `wrangler whoami --json` membership"
                    );
                };
                if matches.next().is_some() {
                    bail!(
                        "Cloudflare account name `{requested}` is ambiguous; pass its stable account ID instead"
                    );
                }
                return Ok(Some(first.clone()));
            }

            if let Some(registered_account_id) = registered_account_id {
                let account = accounts
                    .iter()
                    .find(|account| account.id == registered_account_id)
                    .ok_or_else(|| {
                        anyhow!(
                            "registered Cloudflare account `{registered_account_id}` is not available to the current Wrangler identity; authenticate the correct account or pass `--cloudflare-account <id-or-name>`"
                        )
                    })?;
                return Ok(Some(account.clone()));
            }

            match accounts.as_slice() {
                [account] => Ok(Some(account.clone())),
                many => {
                    let choices = many
                        .iter()
                        .map(|account| format!("{} ({})", account.name, account.id))
                        .collect::<Vec<_>>()
                        .join(", ");
                    bail!(
                        "multiple Cloudflare accounts are available: {choices}. Pass `--cloudflare-account <id-or-name>`; Zodex will not choose interactively."
                    )
                }
            }
        }
    }
}

fn cloudflare_account_fallback_id<'a>(
    registered_account_id: Option<&'a str>,
    environment_account_id: Option<&'a str>,
) -> Option<&'a str> {
    registered_account_id.or(environment_account_id)
}

fn permanent_cloudflare_auth_available(proxy: &OperatorSpriteProxyRecord) -> Result<bool> {
    let deploy = match resolve_proxy_deploy_command() {
        Ok(deploy) => deploy,
        Err(_) => return Ok(false),
    };
    match run_wrangler_whoami(&deploy) {
        Ok(WranglerWhoamiState::Authenticated(accounts)) => Ok(accounts
            .iter()
            .any(|account| account.id == proxy.cloudflare_account_id)),
        Ok(WranglerWhoamiState::Unauthenticated) | Err(_) => Ok(false),
    }
}

fn run_wrangler_deploy_attempt(
    deploy: &ProxyDeployCommandSpec,
    project_dir: &Path,
    config_path: &Path,
    cloudflare_account_id: Option<&str>,
    temporary: bool,
) -> Result<WranglerDeployAttemptOutput> {
    let output_name = if temporary {
        "wrangler-temporary-output.ndjson"
    } else {
        "wrangler-output.ndjson"
    };
    let structured_path = project_dir.join(output_name);
    let _ = fs::remove_file(&structured_path);

    let mut args = deploy.base_args.clone();
    args.push("deploy".to_string());
    if temporary {
        args.push("--temporary".to_string());
    }
    args.extend([
        "--config".to_string(),
        config_path.display().to_string(),
    ]);

    let mut command = Command::new(&deploy.program);
    command
        .args(&args)
        .current_dir(project_dir)
        .env("WRANGLER_OUTPUT_FILE_PATH", &structured_path)
        .env("FORCE_COLOR", "0");
    if temporary {
        command.env_remove("CLOUDFLARE_ACCOUNT_ID");
    } else if let Some(account) = cloudflare_account_id {
        command.env("CLOUDFLARE_ACCOUNT_ID", account);
    } else {
        command.env_remove("CLOUDFLARE_ACCOUNT_ID");
    }
    let output = command
        .output()
        .with_context(|| format!("failed to run {}", deploy.program))?;
    Ok(WranglerDeployAttemptOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        structured: fs::read_to_string(&structured_path).ok(),
    })
}

fn wrangler_attempt_failure_message(output: &WranglerDeployAttemptOutput) -> String {
    let structured_error = output
        .structured
        .as_deref()
        .and_then(|raw| parse_wrangler_deploy_output(raw).err())
        .map(|error| error.to_string());
    let mut sections = Vec::new();
    if let Some(error) = structured_error {
        sections.push(error);
    } else {
        sections.push("Wrangler deployment failed".to_string());
    }
    if !output.stdout.trim().is_empty() {
        sections.push(output.stdout.trim().to_string());
    }
    if !output.stderr.trim().is_empty() {
        sections.push(output.stderr.trim().to_string());
    }
    sanitize_wrangler_output(&sections.join("\n"))
}

fn wrangler_failure_offers_temporary_deploy(output: &WranglerDeployAttemptOutput) -> bool {
    let text = format!("{}\n{}", output.stdout, output.stderr).to_ascii_lowercase();
    text.contains("--temporary")
        && (text.contains("continue without logging in")
            || text.contains("without logging in")
            || text.contains("temporary preview account"))
}

fn sanitize_wrangler_output(output: &str) -> String {
    redact_cloudflare_claim_urls(&redact_api_key_query_params(output))
}

fn redact_cloudflare_claim_urls(output: &str) -> String {
    const CLAIM_PREFIX: &str = "https://dash.cloudflare.com/claim-preview";
    let mut result = String::with_capacity(output.len());
    let mut rest = output;
    while let Some(offset) = rest.find(CLAIM_PREFIX) {
        result.push_str(&rest[..offset]);
        let claim_and_rest = &rest[offset..];
        let end = claim_and_rest
            .find(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '"' | '\'' | ')' | '>' | ']'))
            .unwrap_or(claim_and_rest.len());
        result.push_str("[REDACTED_CLOUDFLARE_CLAIM_URL]");
        rest = &claim_and_rest[end..];
    }
    result.push_str(rest);
    result
}

fn extract_urls(output: &str) -> Vec<String> {
    output
        .split_ascii_whitespace()
        .filter_map(|token| {
            let trimmed = token.trim_matches(|ch: char| {
                matches!(ch, '"' | '\'' | '(' | ')' | '[' | ']' | '<' | '>' | ',' | ';')
            });
            if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
                return None;
            }
            let trimmed = trimmed.trim_end_matches(['.', ',']);
            Url::parse(trimmed).ok().map(|_| trimmed.to_string())
        })
        .collect()
}

fn extract_cloudflare_claim_url(output: &str) -> Option<String> {
    extract_urls(output).into_iter().find(|candidate| {
        Url::parse(candidate).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str() == Some("dash.cloudflare.com")
                && url.path().starts_with("/claim-preview")
                && url.query_pairs().any(|(key, value)| key == "claimToken" && !value.is_empty())
        })
    })
}

fn normalize_worker_url(url: &str) -> Result<String> {
    let mut parsed = Url::parse(url).with_context(|| format!("invalid Worker target URL `{url}`"))?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        bail!("Worker target must be an HTTPS URL: {url}");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        bail!("Worker target must not contain a query or fragment: {url}");
    }
    let normalized_path = parsed.path().trim_end_matches('/').to_string();
    parsed.set_path(&normalized_path);
    Ok(parsed.to_string().trim_end_matches('/').to_string())
}

fn primary_worker_target(metadata: &WranglerDeployMetadata) -> Result<String> {
    metadata
        .targets
        .iter()
        .find_map(|target| normalize_worker_url(target).ok())
        .ok_or_else(|| anyhow!("Wrangler deploy record did not contain a valid HTTPS Worker target"))
}

fn temporary_deployment_from_output(output: &WranglerDeployAttemptOutput) -> Result<TemporaryCloudflareDeployment> {
    let combined = format!("{}\n{}", output.stdout, output.stderr);
    let metadata = output
        .structured
        .as_deref()
        .and_then(|raw| parse_wrangler_deploy_output(raw).ok());
    let worker_url = match metadata.as_ref() {
        Some(metadata) => primary_worker_target(metadata)?,
        None => extract_urls(&combined)
            .into_iter()
            .find(|candidate| {
                Url::parse(candidate).is_ok_and(|url| {
                    url.scheme() == "https"
                        && url.host_str().is_some_and(|host| host.ends_with(".workers.dev"))
                })
            })
            .ok_or_else(|| anyhow!("temporary Wrangler deployment did not expose a Worker URL"))?,
    };
    let claim_url = extract_cloudflare_claim_url(&combined)
        .ok_or_else(|| anyhow!("temporary Wrangler deployment did not expose its claim URL"))?;
    Ok(TemporaryCloudflareDeployment {
        worker_url: normalize_worker_url(&worker_url)?,
        claim_url,
        worker_name: metadata.as_ref().map(|metadata| metadata.worker_name.clone()),
        worker_version: metadata.as_ref().map(|metadata| metadata.version_id.clone()),
    })
}

fn registered_proxy_for_resolution(
    resolution: &ProxyOriginResolution,
) -> Result<Option<OperatorSpriteProxyRecord>> {
    let Some(sprite) = resolution.sprite.as_ref() else {
        return Ok(None);
    };
    Ok(load_operator_sprite_record(sprite)?.and_then(|record| record.proxy))
}

fn verified_current_registered_proxy_url(
    resolution: &ProxyOriginResolution,
) -> Result<Option<String>> {
    let Some(proxy) = registered_proxy_for_resolution(resolution)? else {
        return Ok(None);
    };
    let expected_build = proxy_worker_build_id();
    if proxy.worker_build != expected_build {
        return Ok(None);
    }
    let Ok(status) = proxy_worker_status(&proxy.worker_url) else {
        return Ok(None);
    };
    if !registered_proxy_matches_live_status(&proxy, &status, resolution, &expected_build) {
        return Ok(None);
    }
    Ok(Some(proxy.worker_url))
}

fn registered_proxy_matches_live_status(
    proxy: &OperatorSpriteProxyRecord,
    status: &ProxyWorkerStatus,
    resolution: &ProxyOriginResolution,
    expected_build: &str,
) -> bool {
    proxy.worker_build == expected_build
        && proxy_worker_build_state(status, expected_build) == "current"
        && status.sprite_origin.as_deref() == Some(resolution.origin.as_str())
}

fn verify_deployed_proxy(url: &str, resolution: &ProxyOriginResolution, expected_build: &str) -> Result<()> {
    let status = proxy_worker_status(url)?;
    if status.component != PROXY_WORKER_COMPONENT {
        bail!("deployed Worker reports unexpected component `{}`", status.component);
    }
    if status.build != expected_build {
        bail!(
            "deployed Worker build mismatch: expected `{expected_build}`, got `{}`",
            status.build
        );
    }
    if status.sprite_origin.as_deref() != Some(resolution.origin.as_str()) {
        bail!("deployed Worker points at a different Sprite origin");
    }
    Ok(())
}

fn deploy_proxy_with_cloudflare_flow(
    resolution: &ProxyOriginResolution,
    worker_name: &str,
    worker_build: &str,
    project_dir: &Path,
    config_path: &Path,
    deploy: &ProxyDeployCommandSpec,
    requested_account: Option<&str>,
) -> Result<CloudflareDeployOutcome> {
    let existing_proxy = registered_proxy_for_resolution(resolution)?;
    let registered_account_id = existing_proxy
        .as_ref()
        .map(|proxy| proxy.cloudflare_account_id.as_str());
    let environment_account = env::var("CLOUDFLARE_ACCOUNT_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let outcome = execute_cloudflare_deploy_flow(
        project_dir,
        config_path,
        deploy,
        requested_account,
        registered_account_id,
        existing_proxy.is_some(),
        environment_account.as_deref(),
    )?;

    match &outcome {
        CloudflareDeployOutcome::Permanent(permanent) => {
            verify_deployed_proxy(&permanent.worker_url, resolution, worker_build)?;
            if let Some(sprite) = resolution.sprite.as_ref() {
                save_operator_sprite_proxy_record(
                    sprite,
                    OperatorSpriteProxyRecord {
                        cloudflare_account_id: permanent.cloudflare_account_id.clone(),
                        worker_name: permanent.metadata.worker_name.clone(),
                        worker_url: permanent.worker_url.clone(),
                        worker_version: permanent.metadata.version_id.clone(),
                        worker_build: worker_build.to_string(),
                        deployed_at: format_epoch_seconds_rfc3339(current_epoch_seconds()?)?,
                    },
                )?;
            } else {
                println!(
                    "registry: not updated (pass `--sprite <name>` with `--origin` to associate this Worker with a Sprite)"
                );
            }
            println!("proxy-deployment: permanent");
            println!(
                "cloudflare-account-id: {}",
                permanent.cloudflare_account_id
            );
            println!("worker-name: {}", permanent.metadata.worker_name);
            println!("worker-url: {}", permanent.worker_url);
            println!("worker-version: {}", permanent.metadata.version_id);
            println!("worker-build: {worker_build}");
            if let Some(version) = permanent.metadata.wrangler_version.as_deref() {
                println!("wrangler-version: {version}");
            }
            println!("proxy-deploy: complete");
        }
        CloudflareDeployOutcome::Temporary(temporary) => {
            println!("proxy-deployment: temporary-unclaimed");
            if let Some(name) = temporary.worker_name.as_deref() {
                println!("worker-name: {name}");
            } else {
                println!("worker-name: {worker_name}");
            }
            println!("worker-url: {}", temporary.worker_url);
            if let Some(version) = temporary.worker_version.as_deref() {
                println!("worker-version: {version}");
            }
            println!("worker-build: {worker_build}");
            println!("claim-url: {}", temporary.claim_url);
            println!("claim-within: 60 minutes");
            println!(
                "provider-state: Zodex does not persist the claim URL; Wrangler may cache temporary deployment state in its own global config directory"
            );
            println!(
                "next: claim the URL above, then run `wrangler login --use-keyring` and rerun `zodex sprite proxy deploy` to register a permanent deployment"
            );
            verify_deployed_proxy(&temporary.worker_url, resolution, worker_build).with_context(
                || {
                    "temporary Worker was deployed and its claim URL is shown above, but front-door verification failed"
                },
            )?;
            println!("proxy-deploy: temporary-awaiting-claim");
        }
    }
    Ok(outcome)
}

fn execute_cloudflare_deploy_flow(
    project_dir: &Path,
    config_path: &Path,
    deploy: &ProxyDeployCommandSpec,
    requested_account: Option<&str>,
    registered_account_id: Option<&str>,
    has_registered_worker: bool,
    environment_account: Option<&str>,
) -> Result<CloudflareDeployOutcome> {
    let fallback_account_id =
        cloudflare_account_fallback_id(registered_account_id, environment_account);
    let whoami = run_wrangler_whoami(deploy)?;
    let wrangler_is_unauthenticated = matches!(&whoami, WranglerWhoamiState::Unauthenticated);
    let selected_account =
        select_cloudflare_account(&whoami, requested_account, fallback_account_id)?;
    let account_id = selected_account.as_ref().map(|account| account.id.as_str());

    let attempt = run_wrangler_deploy_attempt(
        deploy,
        project_dir,
        config_path,
        account_id,
        false,
    )?;
    if attempt.success {
        let structured = attempt.structured.as_deref().ok_or_else(|| {
            anyhow!("Wrangler permanent deployment succeeded without structured deployment output")
        })?;
        let metadata = parse_wrangler_deploy_output(structured)?;
        let worker_url = primary_worker_target(&metadata)?;
        let account_id = selected_account
            .as_ref()
            .map(|account| account.id.clone())
            .or_else(|| environment_account.map(str::to_string))
            .ok_or_else(|| {
                anyhow!(
                    "permanent deployment succeeded but its Cloudflare account ID could not be resolved; rerun with `--cloudflare-account <id-or-name>`"
                )
            })?;
        return Ok(CloudflareDeployOutcome::Permanent(
            PermanentCloudflareDeployment {
                cloudflare_account_id: account_id,
                metadata,
                worker_url,
            },
        ));
    }

    if !wrangler_is_unauthenticated || !wrangler_failure_offers_temporary_deploy(&attempt) {
        bail!("{}", wrangler_attempt_failure_message(&attempt));
    }
    if has_registered_worker {
        bail!(
            "the registered Worker needs permanent Cloudflare credentials for redeploy; Zodex will not replace it with a new temporary deployment. Run `wrangler login --use-keyring`, then retry `zodex sprite proxy deploy`."
        );
    }

    let version = proxy_deploy_runner_version(deploy)?;
    ensure_temporary_wrangler_version(&version)?;
    let temporary_attempt = run_wrangler_deploy_attempt(
        deploy,
        project_dir,
        config_path,
        None,
        true,
    )?;
    if !temporary_attempt.success {
        bail!("{}", wrangler_attempt_failure_message(&temporary_attempt));
    }
    let temporary = temporary_deployment_from_output(&temporary_attempt)?;
    Ok(CloudflareDeployOutcome::Temporary(temporary))
}
