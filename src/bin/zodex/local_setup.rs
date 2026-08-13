use super::local_network::*;
use super::local_tunnel::*;
use super::*;

const LOCAL_ZODEXD_UNIT: &str = include_str!("local_zodexd.service");
const LOCAL_PUBLISHER_UNIT: &str = include_str!("local_zodex_prd.service");
const LOCAL_SETUP_REMOTE_SCRIPT_PATH: &str = "/tmp/zodex-local-setup.sh";
const LOCAL_READER_PEM_TMP_PATH: &str = "/tmp/zodex-local-reader.pem";
const LOCAL_PUBLISHER_PEM_TMP_PATH: &str = "/tmp/zodex-local-publisher.pem";
const LOCAL_NETWORK_SCRIPT_TMP_PATH: &str = "/tmp/zodex-local-network";
const LOCAL_NETWORK_UNIT_TMP_PATH: &str = "/tmp/zodex-local-network.service";

#[derive(Debug, Clone)]
pub(super) struct LocalSetupOptions<'a> {
    pub(super) repo: &'a str,
    pub(super) reader_app_id: u64,
    pub(super) reader_pem: &'a Path,
    pub(super) publisher_app_id: u64,
    pub(super) publisher_pem: &'a Path,
    pub(super) default_base: &'a str,
    pub(super) tunnel_id: &'a str,
    pub(super) tunnel_runtime_key: &'a Path,
    pub(super) cpus: Option<u32>,
    pub(super) memory: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalSetupAction {
    Create,
    Reconcile,
    RejectUnmanaged,
}

pub(super) fn classify_local_setup_action(
    state: Option<&LocalTargetRecord>,
    machine: Option<&AppleMachineInspect>,
) -> LocalSetupAction {
    match (state, machine) {
        (None, None) => LocalSetupAction::Create,
        (None, Some(_)) => LocalSetupAction::RejectUnmanaged,
        (Some(_), None) => LocalSetupAction::Create,
        (Some(_), Some(_)) => LocalSetupAction::Reconcile,
    }
}

pub(super) fn validate_local_repo(repo: &str) -> Result<String> {
    let normalized =
        normalize_github_repo(repo).ok_or_else(|| anyhow!("repo must be in owner/repo form"))?;
    let valid = normalized.split('/').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    });
    if !valid {
        bail!("repo contains unsupported characters; use a normal GitHub owner/repo name");
    }
    Ok(normalized)
}

pub(super) fn validate_local_default_base(default_base: &str) -> Result<()> {
    let valid = !default_base.is_empty()
        && !default_base.starts_with('-')
        && !default_base.contains("..")
        && default_base
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'));
    if !valid {
        bail!("default base contains unsupported Git ref characters");
    }
    Ok(())
}

fn validate_local_setup_file(path: &Path, label: &str) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read {label} at {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{label} must be a regular file: {}", path.display());
    }
    fs::File::open(path).with_context(|| format!("{label} is not readable: {}", path.display()))?;
    Ok(())
}

fn validate_local_setup_options(options: &LocalSetupOptions<'_>) -> Result<String> {
    let repo = validate_local_repo(options.repo)?;
    validate_local_default_base(options.default_base)?;
    validate_local_setup_file(options.reader_pem, "reader PEM")?;
    validate_local_setup_file(options.publisher_pem, "publisher PEM")?;
    validate_local_tunnel_id(options.tunnel_id)?;
    validate_local_setup_file(options.tunnel_runtime_key, "tunnel runtime key")?;
    if fs::metadata(options.tunnel_runtime_key)
        .with_context(|| {
            format!(
                "failed to inspect tunnel runtime key at {}",
                options.tunnel_runtime_key.display()
            )
        })?
        .len()
        == 0
    {
        bail!("tunnel runtime key file must not be empty");
    }
    if matches!(options.cpus, Some(0)) {
        bail!("Local CPU count must be greater than zero");
    }
    if options
        .memory
        .is_some_and(|memory| memory.trim().is_empty())
    {
        bail!("Local memory override must not be empty");
    }
    Ok(repo)
}

pub(super) fn local_target_record(
    options: &LocalSetupOptions<'_>,
    repo: String,
    reader_installation_id: u64,
    publisher_installation_id: u64,
    setup_state: LocalSetupState,
) -> LocalTargetRecord {
    LocalTargetRecord {
        version: 1,
        machine_id: LOCAL_MACHINE_NAME.to_string(),
        setup_state,
        image_reference: Some(LOCAL_MACHINE_IMAGE.to_string()),
        requested_cpus: options.cpus,
        requested_memory: options.memory.map(str::to_string),
        network: Some(expected_local_network()),
        setup_sources: Some(LocalSetupSources {
            repo,
            reader_app_id: options.reader_app_id,
            reader_pem_path: options.reader_pem.display().to_string(),
            reader_installation_id,
            publisher_app_id: options.publisher_app_id,
            publisher_pem_path: options.publisher_pem.display().to_string(),
            publisher_installation_id,
            default_base: options.default_base.to_string(),
            tunnel_id: Some(options.tunnel_id.to_string()),
            tunnel_runtime_key_path: Some(options.tunnel_runtime_key.display().to_string()),
        }),
    }
}

pub(super) fn build_local_guest_setup_script(sources: &LocalSetupSources) -> Result<String> {
    let tunnel_id = sources
        .tunnel_id
        .as_deref()
        .ok_or_else(|| anyhow!("Local setup sources are missing tunnel ID"))?;
    let tunnel_fragment = build_local_tunnel_install_fragment(tunnel_id)?;
    Ok(format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

REPO={repo}
DEFAULT_BASE={default_base}
CFG=/etc/zodex/config.toml

if command -v apt-get >/dev/null 2>&1 && \
   (! command -v git >/dev/null 2>&1 || ! command -v unzip >/dev/null 2>&1 || ! command -v python3 >/dev/null 2>&1); then
  apt-get update -y
  apt-get install -y --no-install-recommends git curl ca-certificates sudo unzip python3-minimal
fi

TMP_INSTALLER="$(mktemp)"
curl -fsSL https://zodex.ashray.xyz/install.sh -o "$TMP_INSTALLER"
env \
  ZODEX_INSTALL_MODE=runtime \
  ZODEX_INSTALL_OPERATOR_CLI=0 \
  ZODEX_CONFIG_PATH="$CFG" \
  ZODEX_HTTP_BIND_PORT=8080 \
  ZODEX_AGENT_HOME=/home/zodex-agent \
  ZODEX_DEFAULT_WORKDIR=/workspace \
  bash "$TMP_INSTALLER"
rm -f "$TMP_INSTALLER"

install -d -m 0750 -o root -g zodex /etc/zodex/reader /etc/zodex/publisher
install -m 0640 -o root -g zodex {reader_tmp} /etc/zodex/reader/private-key.pem
install -m 0600 -o zodex-publisher -g zodex {publisher_tmp} /etc/zodex/publisher/private-key.pem

if ! getent group zodex-tunnel >/dev/null; then
  groupadd --system zodex-tunnel
fi
if ! id -u zodex-tunnel >/dev/null 2>&1; then
  useradd --system --gid zodex-tunnel --home-dir /var/lib/zodex/tunnel --no-create-home --shell /usr/sbin/nologin zodex-tunnel
fi
install -d -m 0700 -o zodex-tunnel -g zodex-tunnel /var/lib/zodex/tunnel
install -d -m 0750 -o root -g zodex-tunnel /etc/zodex/tunnel
install -d -m 0755 -o root -g root /usr/local/libexec
install -m 0755 -o root -g root {network_script_tmp} {network_script_path}
install -m 0644 -o root -g root {network_unit_tmp} {network_unit_path}
{tunnel_fragment}

awk '
  BEGIN {{seen_bind_host=0; seen_bind=0; inserted_http=0}}
  /^bind_host = / {{ print "bind_host = \"127.0.0.1\""; seen_bind_host=1; next }}
  /^bind_port = / {{
    print "bind_port = 8443"
    if (!inserted_http) {{ print "http_bind_port = 8080"; inserted_http=1 }}
    seen_bind=1
    next
  }}
  /^http_bind_port = / {{next}}
  {{print}}
  END {{
    if (!seen_bind_host) print "bind_host = \"127.0.0.1\""
    if (!seen_bind) {{
      print "bind_port = 8443"
      if (!inserted_http) print "http_bind_port = 8080"
    }}
  }}
' "$CFG" > "$CFG.tmp"
cat "$CFG.tmp" > "$CFG"
rm -f "$CFG.tmp"

tmp_cfg="$(mktemp)"
awk '
  BEGIN {{ skip=0 }}
  /^# BEGIN ZODEX_GH_APPS_MANAGED$/ {{ skip=1; next }}
  /^# END ZODEX_GH_APPS_MANAGED$/ {{ skip=0; next }}
  skip==0 {{ print }}
' "$CFG" > "$tmp_cfg"

cat >> "$tmp_cfg" <<EOF
# BEGIN ZODEX_GH_APPS_MANAGED
reader_app_id = {reader_app_id}
reader_installation_id = {reader_installation_id}
publisher_app_id = {publisher_app_id}

[[publisher_targets]]
id = "$REPO"
repo = "$REPO"
default_base = "$DEFAULT_BASE"
installation_id = {publisher_installation_id}

[[publisher_installations]]
account = "${{REPO%%/*}}"
default_base = "$DEFAULT_BASE"
installation_id = {publisher_installation_id}
# END ZODEX_GH_APPS_MANAGED
EOF
cat "$tmp_cfg" > "$CFG"
rm -f "$tmp_cfg"
chgrp zodex "$CFG"
chmod 0640 "$CFG"

cat > /etc/systemd/system/zodex-prd.service <<'EOF'
{publisher_unit}EOF
cat > /etc/systemd/system/zodexd.service <<'EOF'
{daemon_unit}EOF
systemctl daemon-reload
systemctl stop {tunnel_service} zodexd.service zodex-prd.service 2>/dev/null || true
systemctl enable {network_service}
systemctl restart {network_service}
systemctl enable --now zodex-prd.service zodexd.service

rm -f {reader_tmp} {publisher_tmp} {tunnel_runtime_key_tmp} {network_script_tmp} {network_unit_tmp} {setup_script}
"#,
        repo = shell_escape_single_quotes(&sources.repo),
        default_base = shell_escape_single_quotes(&sources.default_base),
        reader_tmp = LOCAL_READER_PEM_TMP_PATH,
        publisher_tmp = LOCAL_PUBLISHER_PEM_TMP_PATH,
        reader_app_id = sources.reader_app_id,
        reader_installation_id = sources.reader_installation_id,
        publisher_app_id = sources.publisher_app_id,
        publisher_installation_id = sources.publisher_installation_id,
        publisher_unit = LOCAL_PUBLISHER_UNIT,
        daemon_unit = LOCAL_ZODEXD_UNIT,
        network_script_tmp = LOCAL_NETWORK_SCRIPT_TMP_PATH,
        network_script_path = LOCAL_NETWORK_SCRIPT_PATH,
        network_unit_tmp = LOCAL_NETWORK_UNIT_TMP_PATH,
        network_unit_path = LOCAL_NETWORK_UNIT_PATH,
        network_service = LOCAL_NETWORK_SERVICE_NAME,
        tunnel_fragment = tunnel_fragment,
        tunnel_service = LOCAL_TUNNEL_SERVICE_NAME,
        tunnel_runtime_key_tmp = LOCAL_TUNNEL_RUNTIME_KEY_TMP_PATH,
        setup_script = LOCAL_SETUP_REMOTE_SCRIPT_PATH,
    ))
}

fn provision_local_guest(
    record: &LocalTargetRecord,
    reader_pem: &Path,
    publisher_pem: &Path,
    tunnel_runtime_key: &Path,
) -> Result<()> {
    let sources = record
        .setup_sources
        .as_ref()
        .ok_or_else(|| anyhow!("Local setup record is missing source references"))?;
    let script = build_local_guest_setup_script(sources)?;
    write_local_machine_file(LOCAL_SETUP_REMOTE_SCRIPT_PATH, script.as_bytes())?;
    write_local_machine_file(
        LOCAL_NETWORK_SCRIPT_TMP_PATH,
        build_local_network_reconcile_script().as_bytes(),
    )?;
    write_local_machine_file(LOCAL_NETWORK_UNIT_TMP_PATH, local_network_unit().as_bytes())?;
    let tunnel_key = fs::read(tunnel_runtime_key).with_context(|| {
        format!(
            "failed to read tunnel runtime key from {}",
            tunnel_runtime_key.display()
        )
    })?;
    write_local_machine_file(LOCAL_TUNNEL_RUNTIME_KEY_TMP_PATH, &tunnel_key)?;
    write_local_machine_file(
        LOCAL_READER_PEM_TMP_PATH,
        &fs::read(reader_pem).context("failed to read reader PEM for Local provisioning")?,
    )?;
    write_local_machine_file(
        LOCAL_PUBLISHER_PEM_TMP_PATH,
        &fs::read(publisher_pem).context("failed to read publisher PEM for Local provisioning")?,
    )?;
    run_local_machine_exec(&["/bin/bash".into(), LOCAL_SETUP_REMOTE_SCRIPT_PATH.into()])?;
    Ok(())
}

fn verify_local_guest(record: &LocalTargetRecord) -> Result<()> {
    let repo = &record
        .setup_sources
        .as_ref()
        .ok_or_else(|| anyhow!("Local setup record is missing source references"))?
        .repo;
    let expected_network = record
        .network
        .as_ref()
        .ok_or_else(|| anyhow!("Local setup record is missing network policy identity"))?;
    if !local_network_expectation_matches(expected_network) {
        bail!("Local setup record network policy identity does not match this Zodex build");
    }

    run_local_machine_exec(&local_root_network_verify_command())?;
    run_local_machine_exec(&[
        "/bin/bash".into(),
        "-lc".into(),
        format!(
            "set -euo pipefail; systemctl is-active --quiet {network} zodex-prd.service zodexd.service; for service in zodex-prd.service zodexd.service; do pid=\"$(systemctl show --property MainPID --value \"$service\")\"; test \"$pid\" -gt 0; test \"$(ip netns identify \"$pid\")\" = {namespace}; done",
            network = LOCAL_NETWORK_SERVICE_NAME,
            namespace = LOCAL_NETWORK_NAMESPACE,
        ),
    ])?;
    run_local_machine_exec(&[
        "/bin/bash".into(),
        "-lc".into(),
        format!(
            "set -euo pipefail; test -x {binary}; test -s {config}; test -s {key}; test \"$(cat {version_path})\" = {version}; test \"$(stat -c '%U:%G:%a' {key})\" = 'zodex-tunnel:zodex-tunnel:600'; test \"$(stat -c '%U:%G:%a' {config})\" = 'root:zodex-tunnel:640'; systemctl is-enabled {service} >/dev/null 2>&1 && exit 71 || true; systemctl is-active --quiet {service} && exit 72 || true",
            binary = LOCAL_TUNNEL_BINARY_PATH,
            config = LOCAL_TUNNEL_CONFIG_PATH,
            key = LOCAL_TUNNEL_RUNTIME_KEY_PATH,
            version_path = LOCAL_TUNNEL_VERSION_PATH,
            version = shell_escape_single_quotes(LOCAL_TUNNEL_VERSION),
            service = LOCAL_TUNNEL_SERVICE_NAME,
        ),
    ])?;

    run_local_machine_exec(&local_agent_network_exec(&[
        "/bin/bash".into(),
        "-lc".into(),
        "test -w /workspace && touch /workspace/.zodex-write-check && rm /workspace/.zodex-write-check".into(),
    ]))?;
    run_local_machine_exec(&local_agent_network_exec(&[
        "/usr/bin/getent".into(),
        "ahostsv4".into(),
        "github.com".into(),
    ]))?;
    run_local_machine_exec(&local_agent_network_exec(&[
        "git".into(),
        "ls-remote".into(),
        format!("https://github.com/{repo}.git"),
        "HEAD".into(),
    ]))?;
    run_local_machine_exec(&[
        "/bin/bash".into(),
        "-lc".into(),
        "test \"$(stat -c %U /var/lib/zodex/publisher/run)\" = zodex-publisher && test \"$(stat -c %a /var/lib/zodex/publisher/run)\" = 750 && test \"$(stat -c %U /var/lib/zodex/publisher/run/zodex-prd.sock)\" = zodex-publisher && test \"$(stat -c %a /var/lib/zodex/publisher/run/zodex-prd.sock)\" = 660".into(),
    ])?;

    for secret_path in [
        "/etc/zodex/publisher/private-key.pem",
        LOCAL_TUNNEL_RUNTIME_KEY_PATH,
        LOCAL_TUNNEL_CONFIG_PATH,
    ] {
        let result = run_local_machine_exec(&local_agent_network_exec(&[
            "/bin/bash".into(),
            "-lc".into(),
            format!("test -r {secret_path}"),
        ]));
        if result.is_ok() {
            bail!("zodex-agent unexpectedly gained access to {secret_path}");
        }
    }

    if run_local_machine_exec(&local_agent_network_exec(&[
        "/usr/bin/sudo".into(),
        "-n".into(),
        "true".into(),
    ]))
    .is_ok()
    {
        bail!("zodex-agent unexpectedly has passwordless sudo authority");
    }
    if run_local_machine_exec(&local_agent_network_exec(&[
        "/usr/bin/systemctl".into(),
        "start".into(),
        LOCAL_TUNNEL_SERVICE_NAME.into(),
    ]))
    .is_ok()
    {
        bail!("zodex-agent unexpectedly can start the tunnel service");
    }

    if run_local_machine_exec(&local_agent_network_exec(&[
        "/usr/sbin/ip".into(),
        "link".into(),
        "add".into(),
        "zodex-bypass".into(),
        "type".into(),
        "dummy".into(),
    ]))
    .is_ok()
    {
        bail!("zodex-agent unexpectedly has network-administration authority");
    }

    let gateway_probe = format!(
        "if /usr/bin/ping -c 1 -W 1 {gateway} >/dev/null 2>&1; then exit 23; fi",
        gateway = LOCAL_NETWORK_ROOT_GATEWAY
    );
    run_local_machine_exec(&local_agent_network_exec(&[
        "/bin/bash".into(),
        "-lc".into(),
        gateway_probe,
    ]))?;

    run_local_machine_exec(&local_agent_network_exec(&[
        "/usr/bin/curl".into(),
        "-fsS".into(),
        "http://127.0.0.1:8080/health".into(),
    ]))?;
    Ok(())
}

pub(super) async fn local_setup(options: LocalSetupOptions<'_>) -> Result<()> {
    let repo = validate_local_setup_options(&options)?;
    match probe_apple_provider() {
        LocalProviderAvailability::Ready { .. } => {}
        LocalProviderAvailability::Unsupported(reason) => bail!("Local is unsupported: {reason}"),
        LocalProviderAvailability::Missing => bail!("Apple Container CLI is not installed"),
        LocalProviderAvailability::Incompatible(reason) => {
            bail!("Apple Container is incompatible: {reason}")
        }
    }

    let (target_path, _) = local_state_paths()?;
    let existing_state = load_local_target_record(&target_path)?;
    let existing_machine = inspect_local_machine()?;
    if classify_local_setup_action(existing_state.as_ref(), existing_machine.as_ref())
        == LocalSetupAction::RejectUnmanaged
    {
        bail!(
            "an unmanaged `{LOCAL_MACHINE_NAME}` machine already exists; refusing to adopt or overwrite it"
        );
    }

    super::local_lifecycle::local_revoke_access_before_setup()?;

    ensure_apple_container_system_started()?;
    let reader_installation_id =
        resolve_repo_installation_id(options.reader_app_id, options.reader_pem, &repo).await?;
    let publisher_installation_id =
        resolve_repo_installation_id(options.publisher_app_id, options.publisher_pem, &repo)
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

    let mut record = local_target_record(
        &options,
        repo,
        reader_installation_id,
        publisher_installation_id,
        LocalSetupState::Provisioning,
    );
    save_local_target_record(&target_path, &record)?;

    match classify_local_setup_action(existing_state.as_ref(), existing_machine.as_ref()) {
        LocalSetupAction::Create => {
            build_local_machine_image()?;
            create_local_machine(options.cpus, options.memory)?;
        }
        LocalSetupAction::Reconcile => {
            reconcile_local_machine_resources(options.cpus, options.memory)?;
        }
        LocalSetupAction::RejectUnmanaged => unreachable!("unmanaged Local machine rejected above"),
    }

    provision_local_guest(
        &record,
        options.reader_pem,
        options.publisher_pem,
        options.tunnel_runtime_key,
    )?;
    let machine = inspect_local_machine()?
        .ok_or_else(|| anyhow!("Local machine disappeared during setup"))?;
    if classify_local_home_mount(&machine.home_mount) != LocalHomeMountStatus::Isolated {
        bail!("Local machine home mount is not isolated after provisioning");
    }
    verify_local_guest(&record)?;

    record.setup_state = LocalSetupState::Ready;
    save_local_target_record(&target_path, &record)?;
    println!("local-setup: complete");
    Ok(())
}

pub(super) fn local_exec(command: &[String]) -> Result<()> {
    let (target_path, _) = local_state_paths()?;
    let target = load_local_target_record(&target_path)?
        .ok_or_else(|| anyhow!("Zodex Local is not configured; run `zodex local setup` first"))?;
    if target.setup_state != LocalSetupState::Ready {
        bail!("Zodex Local setup is incomplete; repair setup before operator exec");
    }
    let expected_network = target
        .network
        .as_ref()
        .ok_or_else(|| anyhow!("Zodex Local ready state is missing network policy identity"))?;
    if !local_network_expectation_matches(expected_network) {
        bail!("Zodex Local network policy identity has drifted; rerun `zodex local setup`");
    }
    let machine = inspect_local_machine()?
        .ok_or_else(|| anyhow!("configured Local machine `{LOCAL_MACHINE_NAME}` is missing"))?;
    if classify_local_home_mount(&machine.home_mount) != LocalHomeMountStatus::Isolated {
        bail!("Local machine host-home isolation has drifted; refusing operator exec");
    }
    run_local_machine_exec(&[
        "/usr/bin/systemctl".into(),
        "start".into(),
        LOCAL_NETWORK_SERVICE_NAME.into(),
    ])?;
    run_local_machine_exec(&local_root_network_verify_command())?;
    let output = run_local_machine_exec(command)?;
    print!("{output}");
    Ok(())
}
