use super::local_network::{
    LOCAL_NETWORK_NAMESPACE, local_network_expectation_matches, local_root_network_verify_command,
};
use super::local_tunnel::{
    LOCAL_TUNNEL_CONFIG_PATH, LOCAL_TUNNEL_RUNTIME_KEY_PATH, LOCAL_TUNNEL_SERVICE_NAME,
    LOCAL_TUNNEL_VERSION, LOCAL_TUNNEL_VERSION_PATH, local_tunnel_ready_command,
};
use super::*;

const LOCAL_LEASE_LAUNCHD_LABEL: &str = "com.ashray.zodex.local-lease";
const LOCAL_LEASE_LAUNCHD_PLIST: &str = "com.ashray.zodex.local-lease.plist";
const LOCAL_TUNNEL_READY_TIMEOUT: Duration = Duration::from_secs(30);
const LOCAL_TUNNEL_READY_POLL: Duration = Duration::from_millis(500);
const LOCAL_LEASE_WORKER_MAX_SLEEP: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalLeaseView {
    Inactive,
    Active,
    Expired,
    RevocationPending,
    PossiblyActiveRevocationPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalLeaseWorkerDecision {
    Exit,
    Wait(Duration),
    Revoke,
}

pub(super) trait LocalLifecycleRuntime {
    fn now_epoch_seconds(&mut self) -> Result<u64>;
    fn prepare_runtime(&mut self, target: &LocalTargetRecord) -> Result<()>;
    fn start_tunnel(&mut self) -> Result<()>;
    fn stop_tunnel(&mut self) -> Result<()>;
    fn stop_machine(&mut self) -> Result<()>;
    fn arm_supervisor(&mut self, lease: &LocalAccessLease) -> Result<()>;
    fn disarm_supervisor(&mut self) -> Result<()>;
}

struct SystemLocalLifecycleRuntime;

impl LocalLifecycleRuntime for SystemLocalLifecycleRuntime {
    fn now_epoch_seconds(&mut self) -> Result<u64> {
        current_epoch_seconds()
    }

    fn prepare_runtime(&mut self, target: &LocalTargetRecord) -> Result<()> {
        require_ready_local_target(target)?;
        ensure_apple_container_system_started()?;
        let machine = inspect_local_machine()?
            .ok_or_else(|| anyhow!("configured Local machine `{LOCAL_MACHINE_NAME}` is missing"))?;
        if classify_local_home_mount(&machine.home_mount) != LocalHomeMountStatus::Isolated {
            bail!("Local machine host-home isolation has drifted; rerun `zodex local setup`");
        }

        if run_local_machine_exec(&local_root_network_verify_command()).is_err() {
            run_local_machine_exec(&[
                "/bin/bash".into(),
                "-lc".into(),
                format!(
                    "set -euo pipefail; systemctl stop {tunnel} zodexd.service zodex-prd.service 2>/dev/null || true; systemctl restart zodex-local-network.service; systemctl start zodex-prd.service zodexd.service",
                    tunnel = LOCAL_TUNNEL_SERVICE_NAME,
                ),
            ])?;
        } else {
            run_local_machine_exec(&[
                "/usr/bin/systemctl".into(),
                "start".into(),
                "zodex-local-network.service".into(),
                "zodex-prd.service".into(),
                "zodexd.service".into(),
            ])?;
        }
        run_local_machine_exec(&local_root_network_verify_command())?;
        run_local_machine_exec(&[
            "/bin/bash".into(),
            "-lc".into(),
            format!(
                "set -euo pipefail; systemctl is-active --quiet zodex-prd.service zodexd.service; for service in zodex-prd.service zodexd.service; do pid=\"$(systemctl show --property MainPID --value \"$service\")\"; test \"$pid\" -gt 0; test \"$(ip netns identify \"$pid\")\" = {namespace}; done; test -x /usr/local/bin/tunnel-client; test -r {tunnel_config}; test -r {tunnel_key}; test \"$(cat {version_path})\" = {version}; test \"$(stat -c '%U:%G:%a' {tunnel_key})\" = 'zodex-tunnel:zodex-tunnel:600'; test \"$(stat -c '%U:%G:%a' {tunnel_config})\" = 'root:zodex-tunnel:640'; ip netns exec {namespace} curl -fsS http://127.0.0.1:8080/health >/dev/null",
                namespace = LOCAL_NETWORK_NAMESPACE,
                tunnel_config = LOCAL_TUNNEL_CONFIG_PATH,
                tunnel_key = LOCAL_TUNNEL_RUNTIME_KEY_PATH,
                version_path = LOCAL_TUNNEL_VERSION_PATH,
                version = shell_escape_single_quotes(LOCAL_TUNNEL_VERSION),
            ),
        ])?;
        Ok(())
    }

    fn start_tunnel(&mut self) -> Result<()> {
        run_local_machine_exec(&[
            "/usr/bin/systemctl".into(),
            "restart".into(),
            LOCAL_TUNNEL_SERVICE_NAME.into(),
        ])?;
        let started = Instant::now();
        loop {
            if run_local_machine_exec(&local_tunnel_ready_command()).is_ok() {
                break;
            }
            if started.elapsed() >= LOCAL_TUNNEL_READY_TIMEOUT {
                let active = run_local_machine_exec(&[
                    "/usr/bin/systemctl".into(),
                    "is-active".into(),
                    LOCAL_TUNNEL_SERVICE_NAME.into(),
                ])
                .unwrap_or_else(|_| "unknown".to_string());
                bail!(
                    "Secure MCP Tunnel did not become ready within {} seconds (service: {})",
                    LOCAL_TUNNEL_READY_TIMEOUT.as_secs(),
                    active.trim()
                );
            }
            thread::sleep(LOCAL_TUNNEL_READY_POLL);
        }
        run_local_machine_exec(&[
            "/bin/bash".into(),
            "-lc".into(),
            format!(
                "set -euo pipefail; pid=\"$(systemctl show --property MainPID --value {service})\"; test \"$pid\" -gt 0; test \"$(ip netns identify \"$pid\")\" = {namespace}",
                service = LOCAL_TUNNEL_SERVICE_NAME,
                namespace = LOCAL_NETWORK_NAMESPACE,
            ),
        ])?;
        Ok(())
    }

    fn stop_tunnel(&mut self) -> Result<()> {
        let Some(machine) = inspect_local_machine()? else {
            return Ok(());
        };
        if !machine_status_is_running(&machine.status) {
            return Ok(());
        }
        run_local_machine_exec(&[
            "/usr/bin/systemctl".into(),
            "stop".into(),
            LOCAL_TUNNEL_SERVICE_NAME.into(),
        ])?;
        Ok(())
    }

    fn stop_machine(&mut self) -> Result<()> {
        stop_local_machine()
    }

    fn arm_supervisor(&mut self, lease: &LocalAccessLease) -> Result<()> {
        install_local_lease_launch_agent(lease)
    }

    fn disarm_supervisor(&mut self) -> Result<()> {
        remove_local_lease_launch_agent()
    }
}

pub(super) fn parse_local_access_ttl(raw: &str) -> Result<Duration> {
    parse_duration(raw, "Local access TTL")
}

pub(super) fn local_lease_view(lease: Option<&LocalAccessLease>, now: u64) -> LocalLeaseView {
    match lease {
        None => LocalLeaseView::Inactive,
        Some(lease) if lease.active && lease.revocation_pending => {
            LocalLeaseView::PossiblyActiveRevocationPending
        }
        Some(lease) if lease.revocation_pending => LocalLeaseView::RevocationPending,
        Some(lease) if !lease.active => LocalLeaseView::Inactive,
        Some(lease) if lease.expires_at_epoch_seconds <= now => LocalLeaseView::Expired,
        Some(_) => LocalLeaseView::Active,
    }
}

pub(super) fn local_lease_worker_decision(
    lease: Option<&LocalAccessLease>,
    generation: &str,
    now: u64,
) -> LocalLeaseWorkerDecision {
    let Some(lease) = lease else {
        return LocalLeaseWorkerDecision::Exit;
    };
    if lease.generation != generation {
        return LocalLeaseWorkerDecision::Exit;
    }
    if lease.revocation_pending || (lease.active && lease.expires_at_epoch_seconds <= now) {
        return LocalLeaseWorkerDecision::Revoke;
    }
    if !lease.active {
        return LocalLeaseWorkerDecision::Exit;
    }
    let remaining = lease.expires_at_epoch_seconds.saturating_sub(now);
    LocalLeaseWorkerDecision::Wait(Duration::from_secs(remaining).min(LOCAL_LEASE_WORKER_MAX_SLEEP))
}

fn require_ready_local_target(target: &LocalTargetRecord) -> Result<&LocalSetupSources> {
    if target.setup_state != LocalSetupState::Ready {
        bail!("Zodex Local setup is incomplete; rerun `zodex local setup`");
    }
    let network = target
        .network
        .as_ref()
        .ok_or_else(|| anyhow!("Zodex Local ready state is missing network policy identity"))?;
    if !local_network_expectation_matches(network) {
        bail!("Zodex Local network policy identity has drifted; rerun `zodex local setup`");
    }
    let sources = target
        .setup_sources
        .as_ref()
        .ok_or_else(|| anyhow!("Zodex Local setup source references are missing"))?;
    if sources.tunnel_id.as_deref().is_none_or(str::is_empty)
        || sources
            .tunnel_runtime_key_path
            .as_deref()
            .is_none_or(str::is_empty)
    {
        bail!(
            "Zodex Local setup predates Secure MCP Tunnel provisioning; rerun `zodex local setup`"
        );
    }
    Ok(sources)
}

fn new_local_lease(now: u64, ttl: Duration) -> Result<LocalAccessLease> {
    let expires_at_epoch_seconds = now
        .checked_add(ttl.as_secs())
        .ok_or_else(|| anyhow!("Local access TTL expiration overflowed"))?;
    let mut rng = rand::rng();
    Ok(LocalAccessLease {
        version: 1,
        generation: Alphanumeric.sample_string(&mut rng, 24),
        created_at_epoch_seconds: now,
        expires_at_epoch_seconds,
        active: true,
        revocation_pending: false,
    })
}

fn save_revocation_state(
    lease_path: &Path,
    lease: Option<&LocalAccessLease>,
    tunnel_stopped: bool,
    machine_stopped: bool,
) -> Result<()> {
    let Some(mut updated) = lease.cloned() else {
        return Ok(());
    };
    let access_revoked = tunnel_stopped || machine_stopped;
    updated.active = !access_revoked;
    updated.revocation_pending = !machine_stopped || !access_revoked;
    save_local_access_lease(lease_path, &updated)
}

pub(super) fn revoke_local_access_with_runtime<R: LocalLifecycleRuntime>(
    runtime: &mut R,
    lease_path: &Path,
    expected_generation: Option<&str>,
    disarm_on_success: bool,
) -> Result<bool> {
    let lease = load_local_access_lease(lease_path)?;
    if let Some(expected) = expected_generation
        && lease.as_ref().map(|lease| lease.generation.as_str()) != Some(expected)
    {
        return Ok(false);
    }

    let tunnel_result = runtime.stop_tunnel();
    let machine_result = runtime.stop_machine();
    let tunnel_stopped = tunnel_result.is_ok();
    let machine_stopped = machine_result.is_ok();
    save_revocation_state(lease_path, lease.as_ref(), tunnel_stopped, machine_stopped)?;

    if machine_stopped && disarm_on_success {
        runtime.disarm_supervisor()?;
    } else if !machine_stopped && let Some(lease) = load_local_access_lease(lease_path)?.as_ref() {
        let _ = runtime.arm_supervisor(lease);
    }

    if let Err(tunnel_error) = tunnel_result {
        if let Err(machine_error) = machine_result {
            bail!(
                "failed to stop Secure MCP Tunnel ({tunnel_error}); machine stop also failed ({machine_error})"
            );
        }
        bail!(
            "failed to stop Secure MCP Tunnel; machine was stopped as a fail-closed fallback: {tunnel_error}"
        );
    }
    if let Err(machine_error) = machine_result {
        bail!(
            "Secure MCP Tunnel is stopped, but Local machine stop failed; access is revoked and machine-stop reconciliation remains pending: {machine_error}"
        );
    }
    Ok(true)
}

pub(super) fn local_revoke_access_before_setup() -> Result<()> {
    let (_, lease_path) = local_state_paths()?;
    if load_local_access_lease(&lease_path)?.is_none() {
        return Ok(());
    }
    let mut runtime = SystemLocalLifecycleRuntime;
    revoke_local_access_with_runtime(&mut runtime, &lease_path, None, true)
        .context("failed to revoke existing Local access before setup")?;
    Ok(())
}

pub(super) fn start_local_access_with_runtime<R: LocalLifecycleRuntime>(
    runtime: &mut R,
    target: &LocalTargetRecord,
    lease_path: &Path,
    ttl: Duration,
) -> Result<LocalAccessLease> {
    let sources = require_ready_local_target(target)?;
    let now = runtime.now_epoch_seconds()?;
    if matches!(
        local_lease_view(load_local_access_lease(lease_path)?.as_ref(), now),
        LocalLeaseView::Expired
            | LocalLeaseView::RevocationPending
            | LocalLeaseView::PossiblyActiveRevocationPending
    ) {
        revoke_local_access_with_runtime(runtime, lease_path, None, true)
            .context("failed to reconcile previous Local access lease")?;
    }

    if let Err(error) = runtime.prepare_runtime(target) {
        let _ = revoke_local_access_with_runtime(runtime, lease_path, None, true);
        return Err(error).context("failed to prepare Local runtime");
    }
    if let Err(error) = runtime.start_tunnel() {
        let _ = revoke_local_access_with_runtime(runtime, lease_path, None, true);
        return Err(error).context("failed to start Secure MCP Tunnel");
    }

    let lease = new_local_lease(runtime.now_epoch_seconds()?, ttl)?;
    if let Err(error) = save_local_access_lease(lease_path, &lease) {
        let _ = runtime.stop_tunnel();
        let _ = runtime.stop_machine();
        return Err(error).context("failed to persist Local access lease after tunnel readiness");
    }
    if let Err(error) = runtime.arm_supervisor(&lease) {
        let rollback =
            revoke_local_access_with_runtime(runtime, lease_path, Some(&lease.generation), true);
        return match rollback {
            Ok(_) => {
                Err(error).context("failed to arm durable Local TTL supervisor; access was revoked")
            }
            Err(rollback_error) => bail!(
                "failed to arm durable Local TTL supervisor ({error}); fail-closed access rollback also reported: {rollback_error}"
            ),
        };
    }

    let _ = sources;
    Ok(lease)
}

pub(super) fn local_start(ttl_raw: &str) -> Result<()> {
    let ttl = parse_local_access_ttl(ttl_raw)?;
    match probe_apple_provider() {
        LocalProviderAvailability::Ready { .. } => {}
        LocalProviderAvailability::Unsupported(reason) => bail!("Local is unsupported: {reason}"),
        LocalProviderAvailability::Missing => bail!("Apple Container CLI is not installed"),
        LocalProviderAvailability::Incompatible(reason) => {
            bail!("Apple Container is incompatible: {reason}")
        }
    }
    let (target_path, lease_path) = local_state_paths()?;
    let target = load_local_target_record(&target_path)?
        .ok_or_else(|| anyhow!("Zodex Local is not configured; run `zodex local setup` first"))?;
    let tunnel_id = require_ready_local_target(&target)?
        .tunnel_id
        .as_deref()
        .expect("validated tunnel ID")
        .to_string();
    let mut runtime = SystemLocalLifecycleRuntime;
    let lease = start_local_access_with_runtime(&mut runtime, &target, &lease_path, ttl)?;
    println!("MCP access: active");
    println!("Tunnel: {tunnel_id}");
    println!(
        "Expires: {}",
        format_epoch_seconds_rfc3339(lease.expires_at_epoch_seconds)?
    );
    Ok(())
}

pub(super) fn local_stop() -> Result<()> {
    match probe_apple_provider() {
        LocalProviderAvailability::Ready { .. } => {}
        LocalProviderAvailability::Unsupported(reason) => bail!("Local is unsupported: {reason}"),
        LocalProviderAvailability::Missing => bail!("Apple Container CLI is not installed"),
        LocalProviderAvailability::Incompatible(reason) => {
            bail!("Apple Container is incompatible: {reason}")
        }
    }
    let (_, lease_path) = local_state_paths()?;
    let mut runtime = SystemLocalLifecycleRuntime;
    revoke_local_access_with_runtime(&mut runtime, &lease_path, None, true)?;
    println!("MCP access: inactive");
    println!("Machine: stopped");
    Ok(())
}

pub(super) fn local_lease_worker(generation: &str) -> Result<()> {
    if generation.trim().is_empty() {
        bail!("Local lease worker generation must not be empty");
    }
    let (_, lease_path) = local_state_paths()?;
    loop {
        let lease = load_local_access_lease(&lease_path)?;
        let now = current_epoch_seconds()?;
        match local_lease_worker_decision(lease.as_ref(), generation, now) {
            LocalLeaseWorkerDecision::Exit => return Ok(()),
            LocalLeaseWorkerDecision::Wait(duration) => thread::sleep(duration),
            LocalLeaseWorkerDecision::Revoke => {
                let mut runtime = SystemLocalLifecycleRuntime;
                revoke_local_access_with_runtime(
                    &mut runtime,
                    &lease_path,
                    Some(generation),
                    false,
                )?;
                return Ok(());
            }
        }
    }
}

fn local_launch_agents_dir() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME must be set to manage the Local TTL supervisor")?;
    Ok(Path::new(&home).join("Library/LaunchAgents"))
}

fn local_lease_plist_path() -> Result<PathBuf> {
    Ok(local_launch_agents_dir()?.join(LOCAL_LEASE_LAUNCHD_PLIST))
}

fn xml_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(super) fn build_local_lease_launchd_plist(
    executable: &Path,
    home: &Path,
    generation: &str,
) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{executable}</string>
    <string>local</string>
    <string>lease-worker</string>
    <string>--generation</string>
    <string>{generation}</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>
    <string>{home}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ProcessType</key>
  <string>Background</string>
  <key>ThrottleInterval</key>
  <integer>5</integer>
</dict>
</plist>
"#,
        label = LOCAL_LEASE_LAUNCHD_LABEL,
        executable = xml_escape(&executable.display().to_string()),
        generation = xml_escape(generation),
        home = xml_escape(&home.display().to_string()),
    )
}

fn launchd_service_target() -> String {
    format!(
        "gui/{}/{}",
        Uid::current().as_raw(),
        LOCAL_LEASE_LAUNCHD_LABEL
    )
}

fn launchctl(args: &[String]) -> Result<ProviderCommandOutput> {
    command_output("launchctl", args).context("failed to run launchctl")
}

fn launchctl_checked(args: &[String]) -> Result<String> {
    provider_output_result("launchctl", args, launchctl(args)?)
}

fn remove_loaded_launch_agent() -> Result<()> {
    let target = launchd_service_target();
    let print_args = vec!["print".into(), target.clone()];
    let output = launchctl(&print_args)?;
    if !output.success {
        return Ok(());
    }
    launchctl_checked(&["bootout".into(), target])?;
    Ok(())
}

fn install_local_lease_launch_agent(lease: &LocalAccessLease) -> Result<()> {
    let plist_path = local_lease_plist_path()?;
    let directory = plist_path.parent().expect("LaunchAgent plist parent");
    fs::create_dir_all(directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    let executable = env::current_exe().context("failed to locate current zodex executable")?;
    let home = env::var("HOME").context("HOME must be set to manage the Local TTL supervisor")?;
    let plist = build_local_lease_launchd_plist(&executable, Path::new(&home), &lease.generation);
    let mut temp = NamedTempFile::new_in(directory).with_context(|| {
        format!(
            "failed to create temporary LaunchAgent in {}",
            directory.display()
        )
    })?;
    temp.write_all(plist.as_bytes())
        .context("failed to write Local TTL LaunchAgent")?;
    temp.as_file_mut()
        .sync_all()
        .context("failed to sync Local TTL LaunchAgent")?;
    temp.persist(&plist_path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to replace {}", plist_path.display()))?;
    fs::set_permissions(&plist_path, fs::Permissions::from_mode(0o644))
        .with_context(|| format!("failed to secure {}", plist_path.display()))?;

    remove_loaded_launch_agent()?;
    let domain = format!("gui/{}", Uid::current().as_raw());
    launchctl_checked(&["bootstrap".into(), domain, plist_path.display().to_string()])?;
    launchctl_checked(&["print".into(), launchd_service_target()])?;
    Ok(())
}

fn remove_local_lease_launch_agent() -> Result<()> {
    remove_loaded_launch_agent()?;
    let plist_path = local_lease_plist_path()?;
    match fs::remove_file(&plist_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to remove {}", plist_path.display()))
        }
    }
}

pub(super) fn local_tunnel_runtime_status() -> Result<(bool, bool)> {
    let Some(machine) = inspect_local_machine()? else {
        return Ok((false, false));
    };
    if !machine_status_is_running(&machine.status) {
        return Ok((false, false));
    }
    let active = run_local_machine_exec(&[
        "/usr/bin/systemctl".into(),
        "is-active".into(),
        "--quiet".into(),
        LOCAL_TUNNEL_SERVICE_NAME.into(),
    ])
    .is_ok();
    if !active {
        return Ok((false, false));
    }
    let ready = run_local_machine_exec(&local_tunnel_ready_command()).is_ok();
    Ok((true, ready))
}

pub(super) fn local_guest_runtime_status() -> Result<(bool, bool, bool)> {
    let Some(machine) = inspect_local_machine()? else {
        return Ok((false, false, false));
    };
    if !machine_status_is_running(&machine.status) {
        return Ok((false, false, false));
    }
    let publisher_active = run_local_machine_exec(&[
        "/usr/bin/systemctl".into(),
        "is-active".into(),
        "--quiet".into(),
        "zodex-prd.service".into(),
    ])
    .is_ok();
    let daemon_active = run_local_machine_exec(&[
        "/usr/bin/systemctl".into(),
        "is-active".into(),
        "--quiet".into(),
        "zodexd.service".into(),
    ])
    .is_ok();
    let daemon_healthy = daemon_active
        && run_local_machine_exec(&[
            "/usr/sbin/ip".into(),
            "netns".into(),
            "exec".into(),
            LOCAL_NETWORK_NAMESPACE.into(),
            "/usr/bin/curl".into(),
            "-fsS".into(),
            "http://127.0.0.1:8080/health".into(),
        ])
        .is_ok();
    Ok((daemon_active, daemon_healthy, publisher_active))
}
