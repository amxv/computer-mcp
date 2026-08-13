#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalPlatformSupport {
    Supported,
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalProviderAvailability {
    Unsupported(String),
    Missing,
    Incompatible(String),
    Ready { version: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderCommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct AppleSystemVersionInfo {
    version: String,
    #[serde(rename = "appName")]
    app_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalHomeMountStatus {
    Isolated,
    Unsafe(String),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct AppleMachineInspect {
    id: String,
    status: String,
    cpus: u32,
    memory: u64,
    #[serde(rename = "homeMount")]
    home_mount: String,
    #[serde(rename = "diskSize", default)]
    disk_size: Option<u64>,
    #[serde(rename = "ipAddress", default)]
    ip_address: Option<String>,
}

fn classify_local_platform(os: &str, arch: &str) -> LocalPlatformSupport {
    if os != "macos" {
        return LocalPlatformSupport::Unsupported(format!(
            "requires Apple Silicon macOS; current platform is {os}/{arch}"
        ));
    }
    if arch != "aarch64" {
        return LocalPlatformSupport::Unsupported(format!(
            "requires Apple Silicon (aarch64); current architecture is {arch}"
        ));
    }
    LocalPlatformSupport::Supported
}

fn current_local_platform_support() -> LocalPlatformSupport {
    classify_local_platform(env::consts::OS, env::consts::ARCH)
}

fn apple_machine_inspect_args(machine: &str) -> Vec<String> {
    vec!["machine".into(), "inspect".into(), machine.into()]
}

fn apple_machine_create_help_args() -> Vec<String> {
    vec!["machine".into(), "create".into(), "--help".into()]
}

fn local_machine_build_args(containerfile: &Path, context: &Path) -> Vec<String> {
    vec![
        "build".into(),
        "--tag".into(),
        LOCAL_MACHINE_IMAGE.into(),
        "--file".into(),
        containerfile.display().to_string(),
        context.display().to_string(),
    ]
}

fn local_machine_create_args(cpus: Option<u32>, memory: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "machine".into(),
        "create".into(),
        "--no-boot".into(),
        "--name".into(),
        LOCAL_MACHINE_NAME.into(),
        "--home-mount".into(),
        "none".into(),
    ];
    if let Some(cpus) = cpus {
        args.extend(["--cpus".into(), cpus.to_string()]);
    }
    if let Some(memory) = memory {
        args.extend(["--memory".into(), memory.to_string()]);
    }
    args.push(LOCAL_MACHINE_IMAGE.into());
    args
}

fn local_machine_set_args(cpus: Option<u32>, memory: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "machine".into(),
        "set".into(),
        "--name".into(),
        LOCAL_MACHINE_NAME.into(),
        "home-mount=none".into(),
    ];
    if let Some(cpus) = cpus {
        args.push(format!("cpus={cpus}"));
    }
    if let Some(memory) = memory {
        args.push(format!("memory={memory}"));
    }
    args
}

fn local_machine_run_args(command: &[String]) -> Vec<String> {
    let mut args = vec![
        "machine".into(),
        "run".into(),
        "--root".into(),
        "--name".into(),
        LOCAL_MACHINE_NAME.into(),
        "--".into(),
    ];
    args.extend(command.iter().cloned());
    args
}

fn local_machine_stop_args() -> Vec<String> {
    vec!["machine".into(), "stop".into(), LOCAL_MACHINE_NAME.into()]
}

fn local_machine_delete_args() -> Vec<String> {
    vec!["machine".into(), "delete".into(), LOCAL_MACHINE_NAME.into()]
}

fn parse_apple_system_version(raw: &str) -> Result<String> {
    let versions: Vec<AppleSystemVersionInfo> = serde_json::from_str(raw)
        .context("failed to parse `container system version --format json` output")?;
    let cli = versions
        .into_iter()
        .find(|version| version.app_name == "container")
        .ok_or_else(|| anyhow!("Apple Container version output did not include the `container` CLI"))?;
    if cli.version.trim().is_empty() {
        bail!("Apple Container reported an empty CLI version");
    }
    Ok(cli.version)
}

fn parse_apple_machine_inspect(raw: &str) -> Result<AppleMachineInspect> {
    let machines: Vec<AppleMachineInspect> = serde_json::from_str(raw)
        .context("failed to parse `container machine inspect` JSON")?;
    match machines.as_slice() {
        [machine] => Ok(machine.clone()),
        [] => bail!("`container machine inspect` returned no machine records"),
        _ => bail!("`container machine inspect` returned multiple machine records"),
    }
}

fn machine_status_is_running(status: &str) -> bool {
    matches!(status.to_ascii_lowercase().as_str(), "running" | "started")
}

fn classify_local_home_mount(home_mount: &str) -> LocalHomeMountStatus {
    if home_mount == "none" {
        LocalHomeMountStatus::Isolated
    } else {
        LocalHomeMountStatus::Unsafe(home_mount.to_string())
    }
}

fn command_output(program: &str, args: &[String]) -> io::Result<ProviderCommandOutput> {
    let output = Command::new(program).args(args).output()?;
    Ok(ProviderCommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn provider_output_result(program: &str, args: &[String], output: ProviderCommandOutput) -> Result<String> {
    if output.success {
        return Ok(output.stdout);
    }
    let details = if output.stderr.trim().is_empty() {
        output.stdout.trim()
    } else {
        output.stderr.trim()
    };
    bail!("{program} {} failed: {details}", args.join(" "))
}

fn run_container_capture(args: &[String]) -> Result<String> {
    let output = command_output("container", args).context("failed to run Apple Container CLI")?;
    provider_output_result("container", args, output)
}

fn ensure_apple_container_system_started() -> Result<()> {
    let status = Command::new("container")
        .args(["system", "start"])
        .status()
        .context("failed to start Apple Container services")?;
    if status.success() {
        return Ok(());
    }
    bail!("`container system start` failed with status {status}")
}

fn build_local_machine_image() -> Result<()> {
    let context = tempfile::tempdir().context("failed to create Local machine image build context")?;
    let containerfile = context.path().join("Containerfile");
    fs::write(&containerfile, LOCAL_MACHINE_CONTAINERFILE)
        .context("failed to write embedded Local machine Containerfile")?;
    run_container_capture(&local_machine_build_args(&containerfile, context.path()))?;
    Ok(())
}

fn create_local_machine(cpus: Option<u32>, memory: Option<&str>) -> Result<()> {
    run_container_capture(&local_machine_create_args(cpus, memory))?;
    Ok(())
}

fn reconcile_local_machine_resources(cpus: Option<u32>, memory: Option<&str>) -> Result<()> {
    run_container_capture(&local_machine_set_args(cpus, memory))?;
    Ok(())
}

fn run_local_machine_exec(command: &[String]) -> Result<String> {
    if command.is_empty() {
        bail!("Local operator exec command must not be empty");
    }
    run_container_capture(&local_machine_run_args(command))
}

fn stop_local_machine() -> Result<()> {
    let Some(machine) = inspect_local_machine()? else {
        return Ok(());
    };
    if !machine_status_is_running(&machine.status) {
        return Ok(());
    }
    run_container_capture(&local_machine_stop_args())?;
    Ok(())
}

fn delete_local_machine() -> Result<()> {
    if inspect_local_machine()?.is_none() {
        return Ok(());
    }
    run_container_capture(&local_machine_delete_args())?;
    Ok(())
}

fn parse_local_memory_bytes(raw: &str) -> Result<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("Local memory override must not be empty");
    }
    let digit_end = trimmed
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(trimmed.len());
    if digit_end == 0 {
        bail!("Local memory override must start with a positive integer");
    }
    let amount = trimmed[..digit_end]
        .parse::<u64>()
        .context("Local memory override is too large")?;
    if amount == 0 {
        bail!("Local memory override must be greater than zero");
    }
    let suffix = trimmed[digit_end..].to_ascii_lowercase();
    let multiplier = match suffix.as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024_u64.pow(2),
        "g" | "gb" | "gib" => 1024_u64.pow(3),
        "t" | "tb" | "tib" => 1024_u64.pow(4),
        "p" | "pb" | "pib" => 1024_u64.pow(5),
        _ => bail!("Local memory override must use B, K, M, G, T, or P units (for example `32G`)"),
    };
    amount
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow!("Local memory override is too large"))
}

fn local_machine_configuration_needs_restart(
    machine: &AppleMachineInspect,
    cpus: Option<u32>,
    memory: Option<&str>,
) -> Result<bool> {
    if classify_local_home_mount(&machine.home_mount) != LocalHomeMountStatus::Isolated {
        return Ok(true);
    }
    if cpus.is_some_and(|requested| requested != machine.cpus) {
        return Ok(true);
    }
    if let Some(memory) = memory
        && parse_local_memory_bytes(memory)? != machine.memory
    {
        return Ok(true);
    }
    Ok(false)
}

fn local_resource_drift_lines(
    record: &LocalTargetRecord,
    machine: &AppleMachineInspect,
) -> Vec<String> {
    let mut drift = Vec::new();
    if let Some(requested) = record.requested_cpus
        && requested != machine.cpus
    {
        drift.push(format!("CPUs requested {requested}, observed {}", machine.cpus));
    }
    if let Some(requested) = record.requested_memory.as_deref() {
        match parse_local_memory_bytes(requested) {
            Ok(bytes) if bytes != machine.memory => drift.push(format!(
                "memory requested {requested}, observed {}",
                format_bytes(machine.memory)
            )),
            Err(_) => drift.push(format!(
                "memory intent `{requested}` cannot be compared with observed {}",
                format_bytes(machine.memory)
            )),
            _ => {}
        }
    }
    drift
}

fn local_runtime_ready_for_mcp(
    machine_running: bool,
    daemon_healthy: bool,
    tunnel_ready: bool,
    isolation_verified: bool,
) -> bool {
    machine_running && daemon_healthy && tunnel_ready && isolation_verified
}

fn run_local_machine_exec_with_input(command: &[String], input: &[u8]) -> Result<String> {
    if command.is_empty() {
        bail!("Local operator exec command must not be empty");
    }
    let args = local_machine_run_args(command);
    let mut child = Command::new("container")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start Apple Container machine command")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("failed to open Apple Container machine stdin"))?
        .write_all(input)
        .context("failed to stream data into Local machine")?;
    let output = child
        .wait_with_output()
        .context("failed waiting for Apple Container machine command")?;
    provider_output_result(
        "container",
        &args,
        ProviderCommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
    )
}

fn write_local_machine_file(remote_path: &str, contents: &[u8]) -> Result<()> {
    let command = vec![
        "/bin/sh".into(),
        "-c".into(),
        "umask 077; cat > \"$1\"".into(),
        "zodex-write".into(),
        remote_path.into(),
    ];
    run_local_machine_exec_with_input(&command, contents)?;
    Ok(())
}

fn local_machine_atomic_write_command(remote_path: &str) -> Vec<String> {
    vec![
        "/bin/sh".into(),
        "-c".into(),
        "set -eu; umask 077; staging=\"$1.zodex-upload.$$\"; trap 'rm -f -- \"$staging\"' EXIT HUP INT TERM; cat > \"$staging\"; mv -f -- \"$staging\" \"$1\"; trap - EXIT HUP INT TERM".into(),
        "zodex-write-atomic".into(),
        remote_path.into(),
    ]
}

fn write_local_machine_file_atomic(remote_path: &str, contents: &[u8]) -> Result<()> {
    let command = local_machine_atomic_write_command(remote_path);
    run_local_machine_exec_with_input(&command, contents)?;
    Ok(())
}

fn probe_apple_provider_with<F>(platform: LocalPlatformSupport, mut run: F) -> LocalProviderAvailability
where
    F: FnMut(&str, &[String]) -> io::Result<ProviderCommandOutput>,
{
    if let LocalPlatformSupport::Unsupported(reason) = platform {
        return LocalProviderAvailability::Unsupported(reason);
    }

    let version_args = vec!["system".into(), "version".into(), "--format".into(), "json".into()];
    let version_output = match run("container", &version_args) {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return LocalProviderAvailability::Missing,
        Err(error) => return LocalProviderAvailability::Incompatible(format!("failed to run `container`: {error}")),
    };
    if !version_output.success {
        return LocalProviderAvailability::Incompatible(version_output.stderr.trim().to_string());
    }
    let version = match parse_apple_system_version(&version_output.stdout) {
        Ok(version) => version,
        Err(error) => return LocalProviderAvailability::Incompatible(error.to_string()),
    };

    let help_output = match run("container", &apple_machine_create_help_args()) {
        Ok(output) => output,
        Err(error) => return LocalProviderAvailability::Incompatible(format!("failed to inspect machine capabilities: {error}")),
    };
    let help = format!("{}{}", help_output.stdout, help_output.stderr);
    if !help_output.success || !help.contains("--home-mount") || !help.contains("none") {
        return LocalProviderAvailability::Incompatible(
            "Apple Container lacks required `container machine create --home-mount ... none` capability".into(),
        );
    }
    LocalProviderAvailability::Ready { version }
}

fn probe_apple_provider() -> LocalProviderAvailability {
    probe_apple_provider_with(current_local_platform_support(), command_output)
}

fn inspect_local_machine() -> Result<Option<AppleMachineInspect>> {
    let args = apple_machine_inspect_args(LOCAL_MACHINE_NAME);
    let output = Command::new("container")
        .args(&args)
        .output()
        .context("failed to run `container machine inspect`")?;
    if output.status.success() {
        return parse_apple_machine_inspect(&String::from_utf8_lossy(&output.stdout)).map(Some);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if stderr.contains("not found") || stderr.contains("no such") || stderr.contains("does not exist") {
        return Ok(None);
    }
    bail!("failed to inspect Local machine: {}", String::from_utf8_lossy(&output.stderr).trim())
}

fn format_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= GIB && bytes.is_multiple_of(GIB) {
        format!("{} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else {
        format!("{bytes} bytes")
    }
}

fn print_local_status() -> Result<()> {
    let (target_path, lease_path) = local_state_paths()?;
    let target = load_local_target_record(&target_path)?;
    let lease = load_local_access_lease(&lease_path)?;
    let reset_intent = load_local_ready_setup_intent(&local_last_ready_setup_path()?);
    let now = current_epoch_seconds()?;
    let mut machine_exists = false;
    let mut machine_running = false;
    let mut daemon_healthy = false;
    let mut tunnel_active = false;
    let mut tunnel_ready = false;
    let mut isolation_verified = false;

    println!("Local target: {LOCAL_MACHINE_NAME}");
    println!(
        "Configuration: {}",
        match target.as_ref().map(|record| &record.setup_state) {
            None => "not configured",
            Some(LocalSetupState::Provisioning) => "provisioning / needs repair",
            Some(LocalSetupState::Ready) => "ready",
        }
    );
    match reset_intent {
        Ok(Some(_)) => println!("Reset recovery: last-ready setup intent available"),
        Ok(None)
            if matches!(
                target.as_ref().map(|record| &record.setup_state),
                Some(LocalSetupState::Ready)
            ) =>
        {
            println!("Reset recovery: available from current ready configuration");
        }
        Ok(None) => println!("Reset recovery: unavailable until setup reaches ready"),
        Err(error) => println!("Reset recovery: invalid saved intent ({error})"),
    }

    match probe_apple_provider() {
        LocalProviderAvailability::Unsupported(reason) => {
            println!("Provider: unsupported ({reason})");
            println!("Machine: unknown (provider unavailable)");
        }
        LocalProviderAvailability::Missing => {
            println!("Provider: missing (`container` CLI not found)");
            println!("Machine: unknown (provider unavailable)");
        }
        LocalProviderAvailability::Incompatible(reason) => {
            println!("Provider: incompatible ({reason})");
            println!("Machine: unknown (provider unavailable)");
        }
        LocalProviderAvailability::Ready { version } => {
            println!("Provider: ready ({version})");
            match inspect_local_machine()? {
                None => {
                    if matches!(target.as_ref().map(|record| &record.setup_state), Some(LocalSetupState::Ready)) {
                        println!("Machine: missing (configured target drift)");
                    } else {
                        println!("Machine: not found");
                    }
                }
                Some(machine) => {
                    machine_exists = true;
                    let running = machine_status_is_running(&machine.status);
                    machine_running = running;
                    println!("Machine: {} ({})", if running { "running" } else { "stopped" }, machine.status);
                    println!("Resources: {} CPUs, {} memory", machine.cpus, format_bytes(machine.memory));
                    if let Some(record) = target.as_ref() {
                        for drift in local_resource_drift_lines(record, &machine) {
                            println!("Resource drift: {drift}");
                        }
                    }
                    match classify_local_home_mount(&machine.home_mount) {
                        LocalHomeMountStatus::Isolated => {
                            println!("Home mount: none");
                            if target.is_none() {
                                println!("Adoption: unmanaged machine observed; not treated as configured");
                            }
                        }
                        LocalHomeMountStatus::Unsafe(home_mount) => {
                            println!("Home mount: {home_mount}");
                            println!("Isolation: unsafe drift (host home mount must be `none`)");
                        }
                    }
                }
            }
        }
    }

    if machine_running && matches!(target.as_ref().map(|record| &record.setup_state), Some(LocalSetupState::Ready)) {
        match local_lifecycle::local_guest_runtime_status() {
            Ok((daemon_active, healthy, publisher_active)) => {
                daemon_healthy = healthy;
                println!(
                    "Guest services: zodexd={}, zodex-prd={}",
                    if healthy {
                        "healthy"
                    } else if daemon_active {
                        "active / unhealthy"
                    } else {
                        "inactive"
                    },
                    if publisher_active { "active" } else { "inactive" }
                );
            }
            Err(error) => println!("Guest services: unknown ({error})"),
        }
        let target_network_current = target
            .as_ref()
            .and_then(|record| record.network.as_ref())
            .is_some_and(local_network::local_network_expectation_matches);
        if !target_network_current {
            println!("Isolation: drifted (saved network policy does not match this Zodex build)");
        } else {
            match local_lifecycle::local_runtime_isolation_status() {
                Ok(()) => {
                    isolation_verified = true;
                    println!("Isolation: verified");
                }
                Err(error) => println!("Isolation: drifted / needs setup repair ({error})"),
            }
        }
        match local_lifecycle::local_tunnel_runtime_status() {
            Ok((active, ready)) => {
                tunnel_active = active;
                tunnel_ready = ready;
                println!(
                    "Tunnel: {}",
                    if ready {
                        "ready"
                    } else if active {
                        "running / not ready"
                    } else {
                        "inactive"
                    }
                );
            }
            Err(error) => println!("Tunnel: unknown ({error})"),
        }
    } else {
        println!("Guest services: inactive");
        println!("Isolation: inactive / unverified");
        println!("Tunnel: inactive");
    }

    if matches!(target.as_ref().map(|record| &record.setup_state), Some(LocalSetupState::Ready))
        && machine_exists
    {
        println!(
            "Operator exec: {}",
            if machine_running {
                "available (independent of MCP access)"
            } else {
                "available on demand (independent of MCP access)"
            }
        );
    } else if matches!(target.as_ref().map(|record| &record.setup_state), Some(LocalSetupState::Ready)) {
        println!("Operator exec: unavailable (configured machine is missing; rerun `zodex local setup`)");
    } else {
        println!("Operator exec: unavailable (setup not ready)");
    }

    match local_lifecycle::local_lease_view(lease.as_ref(), now) {
        local_lifecycle::LocalLeaseView::Inactive if tunnel_ready => println!(
            "MCP access: unexpected reachability (tunnel is ready without an active lease; run `zodex local stop`)"
        ),
        local_lifecycle::LocalLeaseView::Inactive => println!("MCP access: inactive"),
        local_lifecycle::LocalLeaseView::Active
            if local_runtime_ready_for_mcp(
                machine_running,
                daemon_healthy,
                tunnel_ready,
                isolation_verified,
            ) =>
        {
            let lease = lease.as_ref().expect("active view has lease");
            println!(
                "MCP access: active until {}",
                format_epoch_seconds_rfc3339(lease.expires_at_epoch_seconds)?
            );
        }
        local_lifecycle::LocalLeaseView::Active => {
            let lease = lease.as_ref().expect("active view has lease");
            if tunnel_ready {
                println!(
                    "MCP access: not accepted as active (lease valid until {}, tunnel is ready but runtime/isolation checks are not satisfied)",
                    format_epoch_seconds_rfc3339(lease.expires_at_epoch_seconds)?
                );
            } else {
                println!(
                    "MCP access: inactive (lease valid until {}, but runtime/tunnel/isolation is not ready)",
                    format_epoch_seconds_rfc3339(lease.expires_at_epoch_seconds)?
                );
            }
        }
        local_lifecycle::LocalLeaseView::Expired if tunnel_ready => {
            let lease = lease.as_ref().expect("expired view has lease");
            println!(
                "MCP access: revocation overdue (lease expired at {}, tunnel is still ready)",
                format_epoch_seconds_rfc3339(lease.expires_at_epoch_seconds)?
            );
        }
        local_lifecycle::LocalLeaseView::Expired => {
            let lease = lease.as_ref().expect("expired view has lease");
            println!(
                "MCP access: expired at {} (runtime is not reachable)",
                format_epoch_seconds_rfc3339(lease.expires_at_epoch_seconds)?
            );
        }
        local_lifecycle::LocalLeaseView::RevocationPending => {
            println!("MCP access: inactive (machine-stop reconciliation pending)");
        }
        local_lifecycle::LocalLeaseView::PossiblyActiveRevocationPending => {
            println!(
                "MCP access: revocation pending ({})",
                if tunnel_active {
                    "tunnel may still be reachable"
                } else {
                    "runtime state is not fully reconciled"
                }
            );
        }
    }
    Ok(())
}
const LOCAL_MACHINE_IMAGE: &str = "local/zodex-machine:1";
const LOCAL_MACHINE_CONTAINERFILE: &str = include_str!("local_machine.Containerfile");
