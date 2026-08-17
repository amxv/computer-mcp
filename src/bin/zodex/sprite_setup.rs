fn validate_publisher_client_id(client_id: &str) -> Result<()> {
    if client_id.is_empty()
        || !client_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!(
            "publisher client ID must be the writer GitHub App Client ID from its settings page"
        );
    }
    Ok(())
}

fn validate_default_base_branch(branch: &str) -> Result<()> {
    if branch.trim().is_empty() {
        bail!("default base branch cannot be empty");
    }
    let status = Command::new("git")
        .args(["check-ref-format", "--branch", branch])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to validate default base branch with git")?;
    if !status.success() {
        bail!("default base branch is not a valid Git branch name");
    }
    Ok(())
}

fn toml_string_literal(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn require_public_sprite_url_auth(url_auth: &str) -> Result<()> {
    validate_sprite_url_auth(url_auth)?;
    if url_auth != "public" {
        bail!(
            "the canonical Sprite flow requires URL auth `public` because the Cloudflare Worker must reach the raw Sprite HTTP edge"
        );
    }
    Ok(())
}

fn sprite_create_action(sprite: &str, org: Option<&str>) -> String {
    let sprite = shell_escape_single_quotes(sprite);
    match org {
        Some(org) => format!(
            "sprite create -o {} {sprite} --skip-console",
            shell_escape_single_quotes(org)
        ),
        None => format!("sprite create {sprite} --skip-console"),
    }
}

fn preflight_sprite_setup_target(sprite: &str, org: Option<&str>) -> Result<SpriteUrlInfo> {
    if !command_exists("sprite") {
        bail!("Sprite CLI is unavailable; install/authenticate `sprite` before running setup");
    }
    sprite_url_info(sprite, org).with_context(|| {
        format!(
            "failed to inspect Sprite `{sprite}` before setup. Confirm Sprite CLI authentication and organization membership. If the Sprite does not exist, create it explicitly with `{}`; Zodex will not create or destroy Sprites automatically.",
            sprite_create_action(sprite, org)
        )
    })
}

async fn preflight_sprite_setup(options: &SpriteSetupOptions<'_>) -> Result<(u64, u64)> {
    require_public_sprite_url_auth(options.url_auth)?;
    validate_publisher_client_id(options.publisher_client_id)?;
    let normalized_repo = normalize_github_repo(options.repo)
        .ok_or_else(|| anyhow!("repo must be in owner/repo form: {}", options.repo))?;
    if normalized_repo != options.repo {
        bail!("repo must use canonical owner/repo form: {normalized_repo}");
    }
    if options.reader_app_id == 0 || options.publisher_app_id == 0 {
        bail!("reader and writer GitHub App IDs must be non-zero");
    }
    validate_default_base_branch(options.default_base)?;
    for (label, pem) in [
        ("reader", options.reader_pem),
        ("writer", options.publisher_pem),
    ] {
        if !pem.is_file() {
            bail!("{label} GitHub App PEM does not exist: {}", pem.display());
        }
    }

    preflight_sprite_setup_target(options.sprite, options.org)?;

    let actual_client_id = github_app_client_id(options.publisher_app_id, options.publisher_pem)
        .await
        .context("failed to validate writer GitHub App metadata")?;
    if actual_client_id != options.publisher_client_id {
        bail!(
            "writer GitHub App Client ID mismatch: the supplied writer PEM/App ID reports `{actual_client_id}`, but `--publisher-client-id` was `{}`. Use the Client ID from the same writer App settings page.",
            options.publisher_client_id
        );
    }
    preflight_publisher_device_flow(options.publisher_client_id)
        .await
        .context("writer GitHub App Device Flow preflight failed before Sprite mutation")?;

    let reader_installation_id =
        resolve_repo_installation_id(options.reader_app_id, options.reader_pem, options.repo)
            .await
            .context("reader GitHub App is not installed for the selected repository")?;
    let publisher_installation_id = resolve_repo_installation_id(
        options.publisher_app_id,
        options.publisher_pem,
        options.repo,
    )
    .await
    .context("writer GitHub App is not installed for the selected repository")?;
    mint_reader_installation_token(
        options.reader_app_id,
        options.reader_pem,
        reader_installation_id,
    )
    .await
    .context("reader GitHub App must allow repository contents read access")?;
    mint_publisher_installation_token_with_metadata(
        options.publisher_app_id,
        options.publisher_pem,
        publisher_installation_id,
    )
    .await
    .context(
        "writer GitHub App must allow contents/pull-request/workflow write access for the selected repository",
    )?;
    Ok((reader_installation_id, publisher_installation_id))
}

async fn sprite_setup(options: SpriteSetupOptions<'_>) -> Result<()> {
    let (reader_installation_id, publisher_installation_id) =
        preflight_sprite_setup(&options).await?;
    println!("sprite-setup-preflight: ok");

    let script = build_sprite_setup_script(&SpriteSetupScriptOptions {
        repo: options.repo,
        reader_app_id: options.reader_app_id,
        reader_installation_id,
        publisher_app_id: options.publisher_app_id,
        publisher_client_id: options.publisher_client_id,
        publisher_installation_id,
        default_base: options.default_base,
        remote_config: options.remote_config,
    });
    let mut script_file = NamedTempFile::new().context("failed to create setup temp file")?;
    use std::io::Write as _;
    script_file
        .write_all(script.as_bytes())
        .context("failed to write setup script")?;
    let exec_args = vec![
        "bash".to_string(),
        SPRITE_SETUP_REMOTE_SCRIPT_PATH.to_string(),
    ];
    run_sprite_exec(
        options.sprite,
        options.org,
        &exec_args,
        &[
            (script_file.path(), SPRITE_SETUP_REMOTE_SCRIPT_PATH),
            (options.reader_pem, "/tmp/zodex-reader.pem"),
            (options.publisher_pem, "/tmp/zodex-publisher.pem"),
        ],
    )?;

    let resolved = resolve_remote_sprite(Some(options.sprite), options.org)?;
    reconcile_github_agent_git_for_mode(&resolved)?;

    sync_sprite_services(
        options.sprite,
        options.org,
        options.remote_config,
        true,
        false,
    )?;
    restart_sprite_services(options.sprite, options.org)?;
    verify_sprite_service_contract(options.sprite, options.org, options.remote_config)?;
    verify_publisher_socket_permissions(options.sprite, options.org)?;
    verify_publisher_key_isolation(options.sprite, options.org)?;
    verify_sprite_service_logs(options.sprite, options.org)?;
    verify_agent_git_identity(options.sprite, options.org)?;
    verify_reader_git_access(options.sprite, options.org, options.repo)?;
    let local_health = read_local_sprite_runtime_health(options.sprite, options.org)?;
    set_sprite_url_auth(options.sprite, options.org, "public")?;
    let info = sprite_url_info(options.sprite, options.org)?;
    if info.auth.as_deref() != Some("public") {
        bail!("Sprite URL auth did not reconcile to public after setup");
    }
    let origin = normalize_proxy_origin(
        info.url
            .as_deref()
            .ok_or_else(|| anyhow!("Sprite URL is unavailable after setup"))?,
    )?;
    let raw_health = parse_sprite_runtime_health(&probe_http_json(
        &format!("{origin}/health"),
        "raw Sprite /health",
    )?)?;
    if raw_health.version != local_health.version {
        bail!("raw Sprite /health version does not match the running localhost runtime");
    }
    register_operator_sprite(options.sprite, options.org, options.remote_config)
        .context("runtime is healthy, but the local Sprite registry could not be updated")?;
    println!("sprite-setup-runtime: ready");

    let resolution = resolve_proxy_origin(Some(options.sprite), options.org, None)?;
    let (worker_url, claim_required) = if let Some(worker_url) =
        verified_current_registered_proxy_url(&resolution)?
    {
        println!("proxy-deploy: skipped-current-registered-worker");
        (worker_url, false)
    } else {
        let deployment = match deploy_proxy_for_resolution(&resolution, None, None, false) {
            Ok(deployment) => deployment,
            Err(error) => {
                eprintln!("sprite-setup: partial-runtime-ready-worker-incomplete");
                return Err(error.context(
                    "Sprite runtime is healthy and registered; rerun setup or `zodex sprite proxy deploy` after fixing Cloudflare authentication/deployment",
                ));
            }
        };
        match deployment {
            CloudflareDeployOutcome::Permanent(permanent) => (permanent.worker_url, false),
            CloudflareDeployOutcome::Temporary(temporary) => (temporary.worker_url, true),
        }
    };
    let key = read_remote_sprite_mcp_key(options.sprite, options.org, options.remote_config)?;
    if let Err(error) = verify_worker_mcp_contract(&worker_url, &key).await {
        eprintln!("sprite-setup: partial-runtime-and-worker-ready-mcp-contract-failed");
        return Err(error);
    }
    if claim_required {
        println!("sprite-setup: runtime-ready-worker-live-claim-required");
    } else {
        println!("sprite-setup: complete");
    }
    Ok(())
}

async fn sprite_upgrade(
    sprite: &str,
    org: Option<&str>,
    version: &str,
    repo: Option<&str>,
    url_auth: Option<&str>,
    remote_config: &Path,
) -> Result<()> {
    if let Some(url_auth) = url_auth {
        require_public_sprite_url_auth(url_auth)?;
    }
    preflight_sprite_setup_target(sprite, org)?;

    let repo_arg = repo.unwrap_or("");
    let script = build_sprite_upgrade_script(version, repo_arg, remote_config);
    let mut script_file = NamedTempFile::new().context("failed to create upgrade temp file")?;
    use std::io::Write as _;
    script_file
        .write_all(script.as_bytes())
        .context("failed to write upgrade script")?;

    let exec_args = vec![
        "bash".to_string(),
        SPRITE_UPGRADE_REMOTE_SCRIPT_PATH.to_string(),
    ];
    run_sprite_exec(
        sprite,
        org,
        &exec_args,
        &[(script_file.path(), SPRITE_UPGRADE_REMOTE_SCRIPT_PATH)],
    )?;

    let resolved = resolve_remote_sprite(Some(sprite), org)?;
    reconcile_github_agent_git_for_mode(&resolved)?;

    let installed_version = verify_installed_sprite_release(sprite, org, version)?;
    sync_sprite_services(sprite, org, remote_config, false, false)?;
    restart_sprite_services(sprite, org)?;
    verify_sprite_service_contract(sprite, org, remote_config)?;
    verify_live_sprite_runtime_version(sprite, org, &installed_version)?;
    verify_sprite_service_logs(sprite, org)?;
    verify_agent_git_identity(sprite, org)?;
    if let Some(repo) =
        repo.map(str::to_string)
            .or(derive_remote_target_repo(sprite, org, remote_config)?)
    {
        verify_reader_git_access(sprite, org, &repo)?;
    }
    verify_publisher_socket_permissions(sprite, org)?;
    verify_publisher_key_isolation(sprite, org)?;
    set_sprite_url_auth(sprite, org, "public")?;
    let info = sprite_url_info(sprite, org)?;
    if info.auth.as_deref() != Some("public") {
        bail!("Sprite URL auth did not reconcile to public after upgrade");
    }
    let origin = normalize_proxy_origin(
        info.url
            .as_deref()
            .ok_or_else(|| anyhow!("Sprite URL is unavailable after upgrade"))?,
    )?;
    let raw_health = parse_sprite_runtime_health(&probe_http_json(
        &format!("{origin}/health"),
        "raw Sprite /health",
    )?)?;
    if raw_health.version != installed_version {
        bail!("raw Sprite /health did not expose the upgraded running runtime version");
    }
    register_operator_sprite(sprite, org, remote_config)
        .context("runtime upgrade is healthy, but local Sprite registry update failed")?;

    let resolved = resolve_remote_sprite(Some(sprite), org)?;
    let record = load_operator_sprite_record(&resolved)?.ok_or_else(|| {
        anyhow!("upgraded Sprite was not found in the local registry after registration")
    })?;
    let Some(proxy) = record.proxy.as_ref() else {
        println!("proxy-update-required: unregistered");
        println!("sprite-upgrade: runtime-complete-proxy-deploy-required");
        return Ok(());
    };
    let embedded_build = proxy_worker_build_id();
    if proxy.worker_build != embedded_build {
        if permanent_cloudflare_auth_available(proxy)? {
            println!("proxy-update: embedded Worker changed; redeploying with registered permanent account");
            let resolution = resolve_proxy_origin(Some(sprite), org, None)?;
            if let Err(error) = deploy_proxy_for_resolution(
                &resolution,
                Some(&proxy.worker_name),
                Some(&proxy.cloudflare_account_id),
                false,
            ) {
                eprintln!("sprite-upgrade: runtime-complete-proxy-update-failed");
                return Err(error.context(
                    "runtime upgrade succeeded, but the registered permanent Worker could not be updated",
                ));
            }
        } else {
            println!("proxy-update-required: embedded Worker build changed");
            println!(
                "next: authenticate the registered Cloudflare account and run `zodex sprite proxy deploy --sprite {sprite}`"
            );
            println!("sprite-upgrade: runtime-complete-proxy-update-required");
            return Ok(());
        }
    }
    let record = load_operator_sprite_record(&resolved)?.ok_or_else(|| {
        anyhow!("Sprite registry disappeared while reconciling the Worker")
    })?;
    verify_sprite_end_to_end_health(&resolved, &record).await?;
    println!("sprite-upgrade: complete");
    Ok(())
}

struct SpriteSetupScriptOptions<'a> {
    repo: &'a str,
    reader_app_id: u64,
    reader_installation_id: u64,
    publisher_app_id: u64,
    publisher_client_id: &'a str,
    publisher_installation_id: u64,
    default_base: &'a str,
    remote_config: &'a Path,
}

fn build_sprite_setup_script(options: &SpriteSetupScriptOptions<'_>) -> String {
    let SpriteSetupScriptOptions {
        repo,
        reader_app_id,
        reader_installation_id,
        publisher_app_id,
        publisher_client_id,
        publisher_installation_id,
        default_base,
        remote_config,
    } = options;
    let publisher_client_id_toml = toml_string_literal(publisher_client_id);
    let repo_toml = toml_string_literal(repo);
    let default_base_toml = toml_string_literal(default_base);
    let repo_account = repo.split('/').next().unwrap_or(repo);
    let repo_account_toml = toml_string_literal(repo_account);
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

REPO={repo}
CFG={cfg}

if ! command -v git >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update -y
  sudo apt-get install -y --no-install-recommends git curl ca-certificates
fi

TMP_INSTALLER="$(mktemp)"
curl -fsSL https://zodex.ashray.xyz/install.sh -o "$TMP_INSTALLER"
sudo env \
  ZODEX_INSTALL_MODE=runtime \
  ZODEX_CONFIG_PATH="$CFG" \
  ZODEX_SERVICE_PORT=8080 \
  ZODEX_AGENT_HOME=/home/zodex-agent \
  ZODEX_DEFAULT_WORKDIR=/workspace \
  bash "$TMP_INSTALLER"
rm -f "$TMP_INSTALLER"

sudo install -d -m 0750 -o root -g zodex /etc/zodex/reader /etc/zodex/publisher
sudo install -m 0640 -o root -g zodex /tmp/zodex-reader.pem /etc/zodex/reader/private-key.pem
sudo install -m 0600 -o zodex-publisher -g zodex /tmp/zodex-publisher.pem /etc/zodex/publisher/private-key.pem

sudo awk '
  BEGIN {{ seen_service_port=0 }}
  /^service_port = / {{ print "service_port = 8080"; seen_service_port=1; next }}
  {{print}}
  END {{
    if (!seen_service_port) print "service_port = 8080"
  }}
' "$CFG" | sudo tee "$CFG" >/dev/null

sudo awk '
  BEGIN {{ seen_agent_home=0; seen_default_workdir=0 }}
  /^agent_home = / {{ print "agent_home = \"/home/zodex-agent\""; seen_agent_home=1; next }}
  /^default_workdir = / {{ print "default_workdir = \"/workspace\""; seen_default_workdir=1; next }}
  {{ print }}
  END {{
    if (!seen_agent_home) print "agent_home = \"/home/zodex-agent\""
    if (!seen_default_workdir) print "default_workdir = \"/workspace\""
  }}
' "$CFG" | sudo tee "$CFG" >/dev/null

tmp_cfg="$(mktemp)"
tmp_block="$(mktemp)"
sudo awk '
  BEGIN {{ skip=0 }}
  /^# BEGIN ZODEX_GH_APPS_MANAGED$/ {{ skip=1; next }}
  /^# END ZODEX_GH_APPS_MANAGED$/ {{ skip=0; next }}
  skip==0 {{ print }}
' "$CFG" > "$tmp_cfg"

cat > "$tmp_block" <<'EOF'
# BEGIN ZODEX_GH_APPS_MANAGED
reader_app_id = {reader_app_id}
reader_installation_id = {reader_installation_id}
publisher_app_id = {publisher_app_id}
publisher_client_id = {publisher_client_id_toml}

[[publisher_targets]]
id = {repo_toml}
repo = {repo_toml}
default_base = {default_base_toml}
installation_id = {publisher_installation_id}

[[publisher_installations]]
account = {repo_account_toml}
default_base = {default_base_toml}
installation_id = {publisher_installation_id}
# END ZODEX_GH_APPS_MANAGED
EOF

sudo bash -lc 'cat "$1" "$2" > "$3"' -- "$tmp_cfg" "$tmp_block" "$CFG"
rm -f "$tmp_cfg" "$tmp_block"
sudo chgrp zodex "$CFG"
sudo chmod 0640 "$CFG"

helper_cmd="/usr/local/bin/zodex-agent --config $CFG git-credential-helper"
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global --replace-all credential.https://github.com.helper "$helper_cmd"
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global credential.https://github.com.useHttpPath true
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global user.name "Zodex Agent"
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global user.email "zodex-agent@local.invalid"

sudo -u zodex-agent env HOME=/home/zodex-agent bash -lc '
  cd /workspace
  test -w /workspace
  touch .zodex-write-check
  rm -f .zodex-write-check
'

sudo -u zodex-agent env HOME=/home/zodex-agent bash -lc '
  smoke_dir=/workspace/.git-identity-smoke
  rm -rf "$smoke_dir"
  git init -q "$smoke_dir"
  cd "$smoke_dir"
  printf "sprite git identity smoke\n" > smoke.txt
  git add smoke.txt
  git commit -q -m "Smoke: verify default agent git identity"
  cd /workspace
  rm -rf "$smoke_dir"
'

sudo -u zodex-agent env HOME=/home/zodex-agent \
  git -C /workspace ls-remote "https://github.com/$REPO.git" HEAD >/dev/null

if sudo -u zodex-agent env HOME=/home/zodex-agent \
  bash -lc 'cat /etc/zodex/publisher/private-key.pem >/dev/null 2>&1'; then
  echo "agent unexpectedly gained publisher key access" >&2
  exit 1
fi

sudo bash -lc 'pkill -f -- "/usr/local/bin/zodexd --config $1" || true; pkill -f -- "/usr/local/bin/zodex-prd --config $1" || true' -- "$CFG"
rm -f /tmp/zodex-reader.pem /tmp/zodex-publisher.pem {setup_script}
"#,
        repo = shell_escape_single_quotes(repo),
        cfg = shell_escape_single_quotes(&remote_config.display().to_string()),
        reader_app_id = reader_app_id,
        reader_installation_id = reader_installation_id,
        publisher_app_id = publisher_app_id,
        publisher_client_id_toml = publisher_client_id_toml,
        publisher_installation_id = publisher_installation_id,
        repo_toml = repo_toml,
        default_base_toml = default_base_toml,
        repo_account_toml = repo_account_toml,
        setup_script = SPRITE_SETUP_REMOTE_SCRIPT_PATH,
    )
}

fn build_sprite_upgrade_script(version: &str, repo: &str, remote_config: &Path) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

CFG={cfg}
VERSION={version}
TARGET_REPO={repo}
INSTALLER_REF="$VERSION"
if [[ "$VERSION" == "latest" ]]; then
  INSTALLER_REF="main"
fi

if [[ ! -f "$CFG" ]]; then
  echo "missing $CFG" >&2
  exit 1
fi

if ! command -v git >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update -y
  sudo apt-get install -y --no-install-recommends git curl ca-certificates
fi

REPO_FOR_INSTALL="amxv/zodex"
if [[ -n "$TARGET_REPO" ]]; then
  REPO_FOR_INSTALL="$TARGET_REPO"
fi

INSTALLER_URL="https://raw.githubusercontent.com/${{REPO_FOR_INSTALL}}/${{INSTALLER_REF}}/scripts/install.sh"
TMP_INSTALLER="$(mktemp)"
curl -fsSL "$INSTALLER_URL" -o "$TMP_INSTALLER"
sudo env \
  ZODEX_REPO="$REPO_FOR_INSTALL" \
  ZODEX_VERSION="$VERSION" \
  ZODEX_SOURCE_REF="$VERSION" \
  ZODEX_INSTALL_MODE=runtime \
  ZODEX_CONFIG_PATH="$CFG" \
  bash "$TMP_INSTALLER"
rm -f "$TMP_INSTALLER"

if [[ -z "$TARGET_REPO" ]]; then
  TARGET_REPO="$(sudo awk -F'"' '/^\[\[publisher_targets\]\]/ {{ in_targets=1; next }} in_targets && /^repo = "/ {{ print $2; exit }}' "$CFG" 2>/dev/null || true)"
fi

helper_cmd="/usr/local/bin/zodex-agent --config $CFG git-credential-helper"
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global --replace-all credential.https://github.com.helper "$helper_cmd"
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global credential.https://github.com.useHttpPath true

current_name="$(sudo -u zodex-agent env HOME=/home/zodex-agent git config --global --get user.name || true)"
current_email="$(sudo -u zodex-agent env HOME=/home/zodex-agent git config --global --get user.email || true)"
if [[ -z "$current_name" ]]; then
  sudo -u zodex-agent env HOME=/home/zodex-agent git config --global user.name "Zodex Agent"
fi
if [[ -z "$current_email" ]]; then
  sudo -u zodex-agent env HOME=/home/zodex-agent git config --global user.email "zodex-agent@local.invalid"
fi

rm -f {upgrade_script}
"#,
        cfg = shell_escape_single_quotes(&remote_config.display().to_string()),
        version = shell_escape_single_quotes(version),
        repo = shell_escape_single_quotes(repo),
        upgrade_script = SPRITE_UPGRADE_REMOTE_SCRIPT_PATH
    )
}
