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

    println!("Local target: {LOCAL_MACHINE_NAME}");
    println!(
        "Configuration: {}",
        match target.as_ref().map(|record| &record.setup_state) {
            None => "not configured",
            Some(LocalSetupState::Provisioning) => "provisioning / needs repair",
            Some(LocalSetupState::Ready) => "ready",
        }
    );

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
                    let running = machine_status_is_running(&machine.status);
                    println!("Machine: {} ({})", if running { "running" } else { "stopped" }, machine.status);
                    println!("Resources: {} CPUs, {} memory", machine.cpus, format_bytes(machine.memory));
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

    match lease {
        Some(lease) if lease.active => {
            let now = current_epoch_seconds()?;
            if lease.expires_at_epoch_seconds <= now {
                println!("MCP access: expired (stale lease state)");
            } else {
                println!("MCP access: active until {}", format_epoch_seconds_rfc3339(lease.expires_at_epoch_seconds)?);
            }
        }
        _ => println!("MCP access: inactive"),
    }
    Ok(())
}
