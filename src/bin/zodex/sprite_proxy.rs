fn derive_remote_target_repo(
    sprite: &str,
    org: Option<&str>,
    remote_config: &Path,
) -> Result<Option<String>> {
    let exec_args = vec![
        "sudo".to_string(),
        "awk".to_string(),
        "-F\"".to_string(),
        r#"/^\[\[publisher_targets\]\]/ { in_targets=1; next } in_targets && /^repo = "/ { print $2; exit }"#.to_string(),
        remote_config.display().to_string(),
    ];
    let raw = run_sprite_exec(sprite, org, &exec_args, &[])?;
    let repo = raw.trim();
    if repo.is_empty() {
        Ok(None)
    } else {
        Ok(Some(repo.to_string()))
    }
}

fn verify_agent_git_identity(sprite: &str, org: Option<&str>) -> Result<()> {
    let script = r#"set -euo pipefail
smoke_dir=/workspace/.git-identity-zodex-smoke
rm -rf "$smoke_dir"
git init -q "$smoke_dir"
cd "$smoke_dir"
printf "sprite git identity smoke\n" > smoke.txt
git add smoke.txt
git commit -q -m "Smoke: verify default agent git identity"
git log -1 --format="%an <%ae>"
cd /workspace
rm -rf "$smoke_dir"
"#;
    let exec_args = vec![
        "sudo".to_string(),
        "-u".to_string(),
        "zodex-agent".to_string(),
        "env".to_string(),
        "HOME=/home/zodex-agent".to_string(),
        "bash".to_string(),
        "-lc".to_string(),
        script.to_string(),
    ];
    run_sprite_exec(sprite, org, &exec_args, &[])?;
    Ok(())
}

fn verify_reader_git_access(sprite: &str, org: Option<&str>, repo: &str) -> Result<()> {
    let exec_args = vec![
        "sudo".to_string(),
        "-u".to_string(),
        "zodex-agent".to_string(),
        "env".to_string(),
        "HOME=/home/zodex-agent".to_string(),
        "git".to_string(),
        "ls-remote".to_string(),
        format!("https://github.com/{repo}.git"),
        "HEAD".to_string(),
    ];
    run_sprite_exec(sprite, org, &exec_args, &[])?;
    Ok(())
}

fn verify_publisher_socket_permissions(sprite: &str, org: Option<&str>) -> Result<()> {
    let script = r#"set -euo pipefail
dir_path=/var/lib/zodex/publisher/run
sock_path=/var/lib/zodex/publisher/run/zodex-prd.sock
[[ "$(stat -c %a "$dir_path")" == "750" ]]
[[ "$(stat -c %U "$dir_path")" == "zodex-publisher" ]]
[[ "$(stat -c %G "$dir_path")" == "zodex" ]]
[[ "$(stat -c %a "$sock_path")" == "660" ]]
[[ "$(stat -c %U "$sock_path")" == "zodex-publisher" ]]
[[ "$(stat -c %G "$sock_path")" == "zodex" ]]
"#;
    let exec_args = vec![
        "sudo".to_string(),
        "bash".to_string(),
        "-lc".to_string(),
        script.to_string(),
    ];
    run_sprite_exec(sprite, org, &exec_args, &[])?;
    Ok(())
}

fn verify_publisher_key_isolation(sprite: &str, org: Option<&str>) -> Result<()> {
    let script = r#"cat /etc/zodex/publisher/private-key.pem >/dev/null 2>&1"#;
    let exec_args = vec![
        "sudo".to_string(),
        "-u".to_string(),
        "zodex-agent".to_string(),
        "env".to_string(),
        "HOME=/home/zodex-agent".to_string(),
        "bash".to_string(),
        "-lc".to_string(),
        script.to_string(),
    ];
    match run_sprite_exec(sprite, org, &exec_args, &[]) {
        Ok(_) => bail!(
            "zodex-agent unexpectedly gained read access to /etc/zodex/publisher/private-key.pem"
        ),
        Err(_) => Ok(()),
    }
}

fn verify_sprite_health(sprite: &str, org: Option<&str>, url_auth: Option<&str>) -> Result<()> {
    verify_local_sprite_health(sprite, org)?;
    if let Some(url_auth) = url_auth {
        set_sprite_url_auth(sprite, org, url_auth)?;
    }
    let info = sprite_url_info(sprite, org)?;
    if let Some(url) = info.url.as_deref() {
        if info.auth.as_deref() == Some("public") {
            run_command_capture(
                "curl",
                &[
                    "-fsS".to_string(),
                    "--retry".to_string(),
                    "3".to_string(),
                    "--retry-all-errors".to_string(),
                    "--retry-delay".to_string(),
                    "2".to_string(),
                    format!("{}/health", url.trim_end_matches('/')),
                ],
            )?;
        }
        println!("sprite-url: {url}");
        if let Some(host) = url
            .trim_end_matches('/')
            .strip_prefix("https://")
            .or_else(|| url.trim_end_matches('/').strip_prefix("http://"))
        {
            let exec_args = vec![
                "sudo".to_string(),
                AGENT_OPERATOR_BINARY.to_string(),
                "show-url".to_string(),
                "--host".to_string(),
                host.to_string(),
            ];
            let output = run_sprite_exec(sprite, org, &exec_args, &[])?;
            print!("{output}");
        }
    }
    println!("sprite-health: ok");
    Ok(())
}

async fn sprite_setup(options: SpriteSetupOptions<'_>) -> Result<()> {
    validate_sprite_url_auth(options.url_auth)?;
    let reader_installation_id =
        resolve_repo_installation_id(options.reader_app_id, options.reader_pem, options.repo)
            .await?;
    let publisher_installation_id = resolve_repo_installation_id(
        options.publisher_app_id,
        options.publisher_pem,
        options.repo,
    )
    .await?;
    mint_reader_installation_token(
        options.reader_app_id,
        options.reader_pem,
        reader_installation_id,
    )
    .await?;
    mint_publisher_installation_token_with_metadata(
        options.publisher_app_id,
        options.publisher_pem,
        publisher_installation_id,
    )
    .await?;

    let script = build_sprite_setup_script(
        options.repo,
        options.reader_app_id,
        reader_installation_id,
        options.publisher_app_id,
        publisher_installation_id,
        options.default_base,
        options.remote_config,
    );
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

    sync_sprite_services(
        options.sprite,
        options.org,
        options.remote_config,
        true,
        false,
    )?;
    verify_publisher_socket_permissions(options.sprite, options.org)?;
    verify_sprite_service_logs(options.sprite, options.org)?;
    verify_sprite_health(options.sprite, options.org, Some(options.url_auth))?;
    if let Err(err) = register_operator_sprite(options.sprite, options.org, options.remote_config) {
        eprintln!("warning: failed to update local Sprite registry: {err:#}");
    }
    println!("sprite-setup: complete");
    Ok(())
}

fn sprite_upgrade(
    sprite: &str,
    org: Option<&str>,
    version: &str,
    repo: Option<&str>,
    url_auth: Option<&str>,
    remote_config: &Path,
) -> Result<()> {
    if let Some(url_auth) = url_auth {
        validate_sprite_url_auth(url_auth)?;
    }

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

    verify_installed_sprite_release(sprite, org, version)?;
    sync_sprite_services(sprite, org, remote_config, false, false)?;
    verify_sprite_service_logs(sprite, org)?;
    verify_local_sprite_health(sprite, org)?;
    verify_agent_git_identity(sprite, org)?;
    if let Some(repo) =
        repo.map(str::to_string)
            .or(derive_remote_target_repo(sprite, org, remote_config)?)
    {
        verify_reader_git_access(sprite, org, &repo)?;
    }
    verify_publisher_socket_permissions(sprite, org)?;
    verify_publisher_key_isolation(sprite, org)?;
    verify_sprite_health(sprite, org, url_auth)?;
    if let Err(err) = register_operator_sprite(sprite, org, remote_config) {
        eprintln!("warning: failed to update local Sprite registry: {err:#}");
    }
    println!("sprite-upgrade: complete");
    Ok(())
}

fn build_sprite_setup_script(
    repo: &str,
    reader_app_id: u64,
    reader_installation_id: u64,
    publisher_app_id: u64,
    publisher_installation_id: u64,
    default_base: &str,
    remote_config: &Path,
) -> String {
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
  ZODEX_INSTALL_OPERATOR_CLI=0 \
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

[[publisher_targets]]
id = "{repo_plain}"
repo = "{repo_plain}"
default_base = "{default_base}"
installation_id = {publisher_installation_id}

[[publisher_installations]]
account = "{repo_account}"
default_base = "{default_base}"
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
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global url."zodex::https://github.com/".pushInsteadOf https://github.com/
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
        repo_plain = repo,
        cfg = shell_escape_single_quotes(&remote_config.display().to_string()),
        reader_app_id = reader_app_id,
        reader_installation_id = reader_installation_id,
        publisher_app_id = publisher_app_id,
        publisher_installation_id = publisher_installation_id,
        default_base = default_base,
        setup_script = SPRITE_SETUP_REMOTE_SCRIPT_PATH,
        repo_account = repo.split('/').next().unwrap_or(repo)
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
  ZODEX_INSTALL_OPERATOR_CLI=0 \
  ZODEX_CONFIG_PATH="$CFG" \
  bash "$TMP_INSTALLER"
rm -f "$TMP_INSTALLER"

if [[ -z "$TARGET_REPO" ]]; then
  TARGET_REPO="$(sudo awk -F'"' '/^\[\[publisher_targets\]\]/ {{ in_targets=1; next }} in_targets && /^repo = "/ {{ print $2; exit }}' "$CFG" 2>/dev/null || true)"
fi

helper_cmd="/usr/local/bin/zodex-agent --config $CFG git-credential-helper"
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global --replace-all credential.https://github.com.helper "$helper_cmd"
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global credential.https://github.com.useHttpPath true
sudo -u zodex-agent env HOME=/home/zodex-agent git config --global url."zodex::https://github.com/".pushInsteadOf https://github.com/

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
