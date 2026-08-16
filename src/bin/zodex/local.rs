use std::io::IsTerminal as _;

use zodex::local::{
    HistoryFormat, HistoryQuery, LocalConfig, LocalHistoryReader, LocalPaths, LocalStatusDocument,
    LocalStatusState, RuntimeKey, build_presentation, clear_local_history, ensure_offline_mutation,
    parse_human_duration, render_presentation, run_local_watch, validate_tunnel_id, WatchOptions,
};

#[cfg(target_os = "macos")]
use zodex::local::{
    LocalSetupRequest, LocalSetupService, MacDittoArchiveExtractor, MacKeychainRuntimeKeyStore,
    OfficialTunnelReleaseClient, ProcessTunnelMetadataValidator, RuntimeKeyStore,
    SystemLaunchdController, TunnelArchitecture, paths_from_runtime_bootstrap, run_hidden_runtime,
    start_via_launchd, stop_via_launchd,
};

#[derive(Debug, Subcommand)]
#[command(after_help = "Agent inspection examples:\n  zodex local status --json\n  zodex local history --last 20\n  zodex local watch\n  zodex local watch --agent k7m2\n  zodex local history --agent k7m2 --since 1h\n  zodex local history --workdir /absolute/repo/path\n  zodex local history --id <invocation-id> --raw")]
enum LocalCommand {
    /// Provision Local configuration, credentials, and the managed tunnel client.
    #[command(after_help = "Examples:\n  zodex local setup\n  printf '%s\\n' \"$OPENAI_TUNNEL_RUNTIME_KEY\" | zodex local setup --tunnel-id tunnel_<id> --runtime-key-stdin\n  zodex local setup --tunnel-id tunnel_<id> --runtime-key-env OPENAI_TUNNEL_RUNTIME_KEY\n\nThe OpenAI tunnel runtime key is read from a hidden terminal prompt by default. For automation, pass it via stdin, an environment variable name, or an already-open file descriptor; never put the secret itself on argv.\n\nmacOS privacy: setup does not bypass or configure TCC. Protected folders or app data may later require a normal user-approved Files & Folders or Full Disk Access grant for the effective Zodex runtime identity. Ordinary unprotected workspaces do not require blanket Full Disk Access.")]
    Setup {
        /// Existing OpenAI tunnel ID. If omitted, setup prompts interactively.
        #[arg(long, value_name = "TUNNEL_ID")]
        tunnel_id: Option<String>,
        /// Read the OpenAI tunnel runtime key from standard input.
        #[arg(
            long,
            conflicts_with_all = ["runtime_key_env", "runtime_key_fd"]
        )]
        runtime_key_stdin: bool,
        /// Read the OpenAI tunnel runtime key from the named environment variable.
        #[arg(
            long,
            value_name = "ENV",
            conflicts_with_all = ["runtime_key_stdin", "runtime_key_fd"]
        )]
        runtime_key_env: Option<String>,
        /// Read the OpenAI tunnel runtime key from an already-open file descriptor.
        #[arg(
            long,
            value_name = "FD",
            conflicts_with_all = ["runtime_key_stdin", "runtime_key_env"]
        )]
        runtime_key_fd: Option<u32>,
        /// Generate a new localhost observability bearer instead of reusing the current one.
        #[arg(long)]
        rotate_observability_bearer: bool,
    },
    /// Start the one Mac-wide Local runtime from a repository or workspace.
    #[command(after_help = "Examples:\n  cd ~/code/amxv/zodex && zodex local start\n  zodex local start ~/code/amxv/zodex --ttl 4h\n\nPATH is startup guidance only. Every exec_command/apply_patch still supplies an explicit absolute workdir. ChatGPT caches MCP server instructions, so refresh the Zodex Local app in ChatGPT app settings after changing PATH.")]
    Start {
        /// Start directory published to ChatGPT as the suggested initial explicit workdir.
        path: Option<PathBuf>,
        /// Optional service-wide wall-clock lifetime, for example 30min, 4h, or 2d.
        #[arg(long)]
        ttl: Option<String>,
    },
    /// Inspect Local configuration and runtime/discovery state.
    #[command(after_help = "Examples:\n  zodex local status\n  zodex local status --json")]
    Status {
        /// Print the stable machine-readable Local status envelope.
        #[arg(long)]
        json: bool,
    },
    /// Attach the optional read-only Agent-aware terminal viewer.
    #[command(after_help = "Examples:\n  zodex local watch\n  zodex local watch --agent k7m2\n  zodex local watch --all")]
    Watch {
        /// Open or wait for one four-character Agent ID.
        #[arg(long, conflicts_with = "all")]
        agent: Option<String>,
        /// Watch combined activity from all Agents.
        #[arg(long)]
        all: bool,
    },
    /// Inspect durable Local invocation history without opening the TUI.
    #[command(after_help = "Examples:\n  zodex local history --last 20\n  zodex local history --agent k7m2 --since 1h\n  zodex local history --workdir /absolute/repo/path\n  zodex local history --id <invocation-id> --raw\n  zodex local history --format json\n  zodex local history clear --yes")]
    History {
        /// Show only the most recent N normalized records.
        #[arg(long)]
        last: Option<usize>,
        /// Show records since a duration such as 30min or 2h.
        #[arg(long)]
        since: Option<String>,
        /// Filter by four-character Agent ID.
        #[arg(long)]
        agent: Option<String>,
        /// Filter by normalized absolute workdir.
        #[arg(long)]
        workdir: Option<PathBuf>,
        /// Show one invocation by durable invocation ID.
        #[arg(long)]
        id: Option<String>,
        /// Output format: markdown (default) or json.
        #[arg(long, default_value = "markdown")]
        format: String,
        /// Include exact raw logical tool evidence where applicable.
        #[arg(long)]
        raw: bool,
        #[command(subcommand)]
        command: Option<LocalHistoryCommand>,
    },
    /// Read or change non-secret Local settings.
    Config {
        #[command(subcommand)]
        command: LocalConfigCommand,
    },
    /// Show bounded Local lifecycle/tunnel diagnostic logs.
    #[command(after_help = "Examples:\n  zodex local logs\n  zodex local logs --lines 500")]
    Logs {
        /// Number of trailing log lines to print.
        #[arg(long, default_value_t = 200)]
        lines: usize,
    },
    /// Stop Local and normally spawned processes across all Agents.
    Stop,
    #[command(name = "__runtime", hide = true)]
    Runtime {
        #[arg(long, hide = true)]
        bootstrap: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum LocalConfigCommand {
    /// Print all non-secret Local settings, or one named key.
    #[command(after_help = "Examples:\n  zodex local config get\n  zodex local config get history.max-age")]
    Get { key: Option<String> },
    /// Persist one non-secret setting. Local must be stopped.
    #[command(after_help = "Examples:\n  zodex local config set history.max-age 60d\n  zodex local config set history.max-size 500mb\n  zodex local config set tunnel.id <tunnel-id>")]
    Set { key: String, value: String },
}

#[derive(Debug, Subcommand)]
enum LocalHistoryCommand {
    /// Clear durable Local history. Local must be stopped.
    Clear {
        /// Skip the interactive confirmation.
        #[arg(long)]
        yes: bool,
    },
}

async fn handle_local_command(command: LocalCommand) -> Result<()> {
    if let LocalCommand::Runtime { bootstrap } = &command {
        ensure_local_runtime_host()?;
        return run_native_hidden_runtime(bootstrap.clone()).await;
    }
    let paths = LocalPaths::discover()?;
    match command {
        LocalCommand::Setup {
            tunnel_id,
            runtime_key_stdin,
            runtime_key_env,
            runtime_key_fd,
            rotate_observability_bearer,
        } => {
            ensure_local_runtime_host()?;
            ensure_offline_mutation(&paths, "run Local setup")?;
            let (tunnel_id, runtime_key) = resolve_local_setup_inputs(
                tunnel_id,
                runtime_key_stdin,
                runtime_key_env.as_deref(),
                runtime_key_fd,
            )?;
            run_native_local_setup(
                &paths,
                tunnel_id,
                runtime_key,
                rotate_observability_bearer,
            )
            .await
        }
        LocalCommand::Start { path, ttl } => {
            let start_dir = resolve_local_start_directory(path.as_deref())?;
            let ttl_seconds = ttl
                .as_deref()
                .map(|ttl| {
                    parse_human_duration(ttl)
                        .with_context(|| format!("invalid Local service TTL `{ttl}`"))
                })
                .transpose()?;
            ensure_local_runtime_host()?;
            run_native_local_start(&paths, &start_dir, ttl_seconds).await
        }
        LocalCommand::Status { json } => print_local_status(&paths, json),
        LocalCommand::Watch { agent, all } => {
            if let Some(agent) = agent.as_deref() {
                validate_agent_id(agent)?;
            }
            ensure_local_runtime_host()?;
            run_local_watch(&paths, WatchOptions { agent, all }).await
        }
        LocalCommand::History {
            last,
            since,
            agent,
            workdir,
            id,
            format,
            raw,
            command,
        } => {
            let since_ms = since
                .as_deref()
                .map(|since| {
                    let duration = parse_human_duration(since)
                        .with_context(|| format!("invalid history duration `{since}`"))?;
                    let now_ms = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
                    let delta_ms = i128::from(duration) * 1_000;
                    i64::try_from(now_ms.saturating_sub(delta_ms))
                        .context("history --since timestamp is outside supported range")
                })
                .transpose()?;
            if let Some(agent) = agent.as_deref() {
                validate_agent_id(agent)?;
            }
            if let Some(workdir) = workdir.as_deref()
                && !workdir.is_absolute()
            {
                bail!("history --workdir must be an absolute path");
            }
            let format = HistoryFormat::parse(&format)?;
            if let Some(LocalHistoryCommand::Clear { yes }) = command {
                ensure_offline_mutation(&paths, "clear Local history")?;
                if !yes {
                    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
                        bail!("non-interactive `zodex local history clear` requires --yes");
                    }
                    let answer = prompt_line("Clear all retained Zodex Local history? [y/N]: ")?;
                    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                        println!("Local history clear cancelled");
                        return Ok(());
                    }
                }
                clear_local_history(&paths.history_database())?;
                println!("cleared Local history at {}", paths.history_database().display());
                return Ok(());
            }

            let invocation_id = id
                .as_deref()
                .map(|value| {
                    value
                        .parse::<i64>()
                        .with_context(|| format!("invalid durable invocation ID `{value}`"))
                })
                .transpose()?;
            let mut query = HistoryQuery {
                last: last.unwrap_or(if invocation_id.is_some() { 1 } else { 20 }),
                since_ms,
                active_or_changed_since_ms: None,
                active_process_invocation_ids: Vec::new(),
                agent_id: agent,
                normalized_workdir: None,
                invocation_id,
                include_raw: raw,
            };
            if let Some(workdir) = workdir.as_deref() {
                query = query.with_workdir(workdir)?;
            }
            let records = LocalHistoryReader::query(&paths.history_database(), &query)?;
            if raw {
                print!("{}", LocalHistoryReader::render(&records, format, true)?);
            } else {
                let agents =
                    LocalHistoryReader::agent_summaries(&paths.history_database(), &records)?;
                let presentation = build_presentation(&records, &agents);
                print!("{}", render_presentation(&presentation, format)?);
            }
            Ok(())
        }
        LocalCommand::Config { command } => handle_local_config(&paths, command),
        LocalCommand::Logs { lines } => print_local_logs(&paths, lines),
        LocalCommand::Stop => {
            ensure_local_runtime_host()?;
            run_native_local_stop(&paths).await
        }
        LocalCommand::Runtime { .. } => unreachable!("hidden runtime handled before path discovery"),
    }
}

#[cfg(target_os = "macos")]
async fn run_native_local_start(
    paths: &LocalPaths,
    start_directory: &Path,
    ttl_seconds: Option<u64>,
) -> Result<()> {
    let secrets = MacKeychainRuntimeKeyStore;
    if secrets.get()?.is_none() {
        bail!("OpenAI tunnel runtime key is missing from Keychain; run `zodex local setup` first");
    }
    let config = LocalConfig::load(&paths.config_file())?;
    if !config.is_provider_configured() {
        bail!("Zodex Local is not configured; run `zodex local setup` first");
    }
    let executable = env::current_exe()
        .context("failed to resolve installed Zodex executable")?
        .canonicalize()
        .context("failed to canonicalize installed Zodex executable")?;
    let environment = env::vars_os().collect::<Vec<_>>();
    let launchd = SystemLaunchdController::for_current_user();
    let outcome = start_via_launchd(
        paths,
        &executable,
        start_directory,
        ttl_seconds,
        &environment,
        &launchd,
    )
    .await?;
    let discovery = &outcome.discovery;
    println!(
        "Zodex Local is {}.",
        if outcome.already_running {
            "already running"
        } else {
            "ready"
        }
    );
    println!("Runtime: {}", discovery.runtime_id);
    println!("PID: {}", discovery.pid);
    println!("Start directory: {}", discovery.start_directory.display());
    if let Some(expires_at) = discovery.expires_at.as_deref() {
        println!("Expires: {expires_at}");
    }
    println!(
        "Agents: {} ({} active process{})",
        outcome.current_runtime_agent_count,
        outcome.active_process_count,
        if outcome.active_process_count == 1 { "" } else { "es" }
    );
    println!(
        "ChatGPT: refresh the Zodex Local app in app settings to load this start directory."
    );
    println!("Inspect: zodex local status | zodex local watch");
    println!("Stop: zodex local stop");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
async fn run_native_local_start(
    _paths: &LocalPaths,
    _start_directory: &Path,
    _ttl_seconds: Option<u64>,
) -> Result<()> {
    bail!("Zodex Local start is only available on macOS")
}

#[cfg(target_os = "macos")]
async fn run_native_local_stop(paths: &LocalPaths) -> Result<()> {
    let launchd = SystemLaunchdController::for_current_user();
    let outcome = stop_via_launchd(paths, &launchd).await?;
    println!("Zodex Local stopped ({outcome:?}).");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
async fn run_native_local_stop(_paths: &LocalPaths) -> Result<()> {
    bail!("Zodex Local stop is only available on macOS")
}

#[cfg(target_os = "macos")]
async fn run_native_hidden_runtime(bootstrap: PathBuf) -> Result<()> {
    let paths = paths_from_runtime_bootstrap(&bootstrap)?;
    let secrets = MacKeychainRuntimeKeyStore;
    let runtime_key = secrets.get()?.context(
        "OpenAI tunnel runtime key is missing from Keychain; run `zodex local setup` again",
    )?;
    run_hidden_runtime(paths, bootstrap, runtime_key).await
}

#[cfg(not(target_os = "macos"))]
async fn run_native_hidden_runtime(_bootstrap: PathBuf) -> Result<()> {
    bail!("hidden Zodex Local runtime is only available on macOS")
}

fn resolve_local_setup_inputs(
    tunnel_id: Option<String>,
    runtime_key_stdin: bool,
    runtime_key_env: Option<&str>,
    runtime_key_fd: Option<u32>,
) -> Result<(String, RuntimeKey)> {
    if tunnel_id.is_none() && runtime_key_stdin {
        bail!(
            "--runtime-key-stdin requires --tunnel-id so setup does not mix an interactive tunnel-ID prompt with secret stdin"
        );
    }

    let tunnel_id = match tunnel_id {
        Some(value) => value,
        None => prompt_line("OpenAI tunnel ID: ")?,
    };
    let tunnel_id = tunnel_id.trim().to_string();
    validate_tunnel_id(&tunnel_id)?;

    let raw_key = if runtime_key_stdin {
        read_secret_limited(&mut io::stdin().lock(), "standard input")?
    } else if let Some(variable) = runtime_key_env {
        if variable.is_empty() || variable.contains('=') || variable.contains('\0') {
            bail!("--runtime-key-env must name one environment variable");
        }
        env::var(variable)
            .with_context(|| format!("environment variable `{variable}` is not set or is not valid UTF-8"))?
    } else if let Some(fd) = runtime_key_fd {
        read_runtime_key_from_fd(fd)?
    } else {
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            bail!(
                "non-interactive setup must choose --runtime-key-stdin, --runtime-key-env <ENV>, or --runtime-key-fd <FD>"
            );
        }
        rpassword::prompt_password("OpenAI tunnel runtime key: ")
            .context("failed to read OpenAI tunnel runtime key from terminal")?
    };

    Ok((tunnel_id, RuntimeKey::new(trim_one_line_ending(raw_key))?))
}

fn prompt_line(prompt: &str) -> Result<String> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        bail!("non-interactive setup requires --tunnel-id <TUNNEL_ID>");
    }
    eprint!("{prompt}");
    io::stderr().flush().context("failed to flush setup prompt")?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .context("failed to read setup input")?;
    Ok(value)
}

fn read_secret_limited(reader: &mut impl Read, source: &str) -> Result<String> {
    const LIMIT: u64 = 16 * 1024 + 2;
    let mut bytes = Vec::new();
    reader
        .take(LIMIT)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read OpenAI tunnel runtime key from {source}"))?;
    if bytes.len() as u64 == LIMIT {
        bail!("OpenAI tunnel runtime key from {source} is unexpectedly large");
    }
    String::from_utf8(bytes)
        .with_context(|| format!("OpenAI tunnel runtime key from {source} is not valid UTF-8"))
}

#[cfg(unix)]
fn read_runtime_key_from_fd(fd: u32) -> Result<String> {
    use std::os::fd::BorrowedFd;

    use nix::fcntl::{FcntlArg, FdFlag, fcntl};

    let raw_fd = i32::try_from(fd).context("runtime-key file descriptor is out of range")?;
    // SAFETY: the caller asserts this is an already-open descriptor by choosing
    // --runtime-key-fd. BorrowedFd does not take ownership or close it.
    let borrowed = unsafe { BorrowedFd::borrow_raw(raw_fd) };
    fcntl(borrowed, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))
        .with_context(|| format!("failed to mark runtime-key file descriptor {fd} close-on-exec"))?;
    let path = PathBuf::from(format!("/dev/fd/{fd}"));
    let mut file = fs::File::open(&path)
        .with_context(|| format!("failed to open runtime-key file descriptor {fd}"))?;
    read_secret_limited(&mut file, &format!("file descriptor {fd}"))
}

#[cfg(not(unix))]
fn read_runtime_key_from_fd(_fd: u32) -> Result<String> {
    bail!("--runtime-key-fd is only supported on Unix hosts")
}

fn trim_one_line_ending(mut value: String) -> String {
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    value
}

#[cfg(target_os = "macos")]
async fn run_native_local_setup(
    paths: &LocalPaths,
    tunnel_id: String,
    runtime_key: RuntimeKey,
    rotate_observability_bearer: bool,
) -> Result<()> {
    let releases = OfficialTunnelReleaseClient::new()?;
    let extractor = MacDittoArchiveExtractor;
    let validator = ProcessTunnelMetadataValidator::new();
    let secrets = MacKeychainRuntimeKeyStore;
    let service = LocalSetupService::new(paths, &releases, &extractor, &validator, &secrets);
    let result = service
        .run(LocalSetupRequest {
            tunnel_id,
            runtime_key,
            architecture: TunnelArchitecture::current_macos()?,
            rotate_observability_bearer,
        })
        .await?;

    println!("Zodex Local setup complete.");
    println!("Tunnel: {}", result.tunnel_id);
    println!(
        "Tunnel client: {} ({}, {})",
        result.managed_binary.display(),
        result.release_version,
        if result.binary_updated { "updated" } else { "verified/reused" }
    );
    println!("OpenAI runtime key: stored in macOS Keychain");
    println!("Tunnel metadata: read access verified; Tunnels Use/readiness is verified by `zodex local start`");
    println!(
        "Observability bearer: {}",
        if result.observability_bearer_rotated {
            "generated/rotated"
        } else {
            "verified/reused"
        }
    );
    println!(
        "macOS privacy: setup does not change TCC. Protected folders/app data may later require a normal user-approved Files & Folders or Full Disk Access grant; ordinary unprotected workspaces do not require blanket Full Disk Access."
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
async fn run_native_local_setup(
    _paths: &LocalPaths,
    _tunnel_id: String,
    _runtime_key: RuntimeKey,
    _rotate_observability_bearer: bool,
) -> Result<()> {
    bail!("Zodex Local setup is only available on macOS")
}

fn handle_local_config(paths: &LocalPaths, command: LocalConfigCommand) -> Result<()> {
    match command {
        LocalConfigCommand::Get { key } => {
            let config = LocalConfig::load(&paths.config_file())?;
            if let Some(key) = key {
                println!("{}", config.get(&key)?);
            } else {
                let rendered = toml::to_string_pretty(&config)
                    .context("failed to render Local configuration")?;
                print!("{rendered}");
            }
            Ok(())
        }
        LocalConfigCommand::Set { key, value } => {
            ensure_offline_mutation(paths, "change Local configuration")?;
            let mut config = LocalConfig::load(&paths.config_file())?;
            config.set(&key, &value)?;
            config.save(&paths.config_file())?;
            println!("updated {key} in {}", paths.config_file().display());
            Ok(())
        }
    }
}

fn print_local_status(paths: &LocalPaths, json: bool) -> Result<()> {
    let status = LocalStatusDocument::inspect(paths)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status).context("failed to serialize Local status")?
        );
        return Ok(());
    }

    let state = match status.state {
        LocalStatusState::Unconfigured => "unconfigured",
        LocalStatusState::Stopped => "stopped",
        LocalStatusState::Running => "running",
        LocalStatusState::Stale => "stale runtime state",
    };
    println!("Zodex Local: {state}");
    println!("Config: {}", status.config_path.display());
    println!(
        "History retention: {} / {}",
        status.history.max_age, status.history.max_size
    );
    println!(
        "History store: {} ({} bytes{})",
        status.history.store_state,
        status.history.physical_size_bytes,
        if status.history.over_budget {
            ", over budget"
        } else {
            ""
        }
    );
    if let Some(reason) = &status.history.store_reason {
        println!("History health detail: {reason}");
    }
    if let Some(error) = &status.history.last_retention_error {
        println!("History retention error: {error}");
    }
    println!("Discovery: {}", status.discovery_path.display());
    if let Some(runtime) = status.runtime.as_ref() {
        println!("Runtime ID: {}", runtime.runtime_id);
        println!("Lifecycle: {:?}", runtime.lifecycle);
        if let Some(process) = runtime.process.as_ref() {
            println!("Runtime PID: {}", process.pid);
        }
        if let Some(start_directory) = runtime.start_directory.as_ref() {
            println!("Start directory: {}", start_directory.display());
        }
        if let Some(started_at) = runtime.started_at.as_deref() {
            println!("Started: {started_at}");
        }
        if let Some(expires_at) = runtime.expires_at.as_deref() {
            println!("Expires: {expires_at}");
        }
        println!(
            "MCP: {}",
            if runtime.health.mcp_ready { "ready" } else { "not ready" }
        );
        println!(
            "Tunnel: process={} control-plane={} ready={}",
            runtime.health.tunnel_process_running,
            runtime.health.tunnel_control_plane_ready,
            runtime.health.tunnel_ready
        );
        println!(
            "Observability: {}",
            if runtime.health.observability_ready {
                "ready"
            } else {
                "not ready"
            }
        );
        if let Some(error) = runtime.health.last_error.as_deref() {
            println!("Runtime health detail: {error}");
        }
    }
    println!("Current-runtime Agents: {}", status.current_runtime_agent_count);
    println!("Active processes: {}", status.active_process_count);
    if let Some(discovery) = status.discovery.as_ref() {
        println!("Observer: {}", discovery.observability.base_url);
    } else {
        println!("Runtime discovery: inactive");
    }
    match status.state {
        LocalStatusState::Unconfigured => println!("Next: zodex local setup"),
        LocalStatusState::Stopped => println!("Next: zodex local start"),
        LocalStatusState::Running => println!("Next: zodex local watch"),
        LocalStatusState::Stale => println!("Next: zodex local stop"),
    }
    Ok(())
}

fn print_local_logs(paths: &LocalPaths, lines: usize) -> Result<()> {
    if lines == 0 {
        return Ok(());
    }
    let current = paths.diagnostic_log_file();
    let rotated = current.with_extension("log.1");
    let mut text = String::new();
    for path in [&rotated, &current] {
        if !path.exists() {
            continue;
        }
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read Local diagnostic log {}", path.display()))?;
        text.push_str(&String::from_utf8_lossy(&bytes));
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    if text.is_empty() {
        println!("No Local lifecycle diagnostics have been recorded yet.");
        return Ok(());
    }
    let all = text.lines().collect::<Vec<_>>();
    let start = all.len().saturating_sub(lines);
    for line in &all[start..] {
        println!("{line}");
    }
    Ok(())
}

fn resolve_local_start_directory(path: Option<&Path>) -> Result<PathBuf> {
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => env::current_dir().context("failed to read current directory for Local start")?,
    };
    let canonical = path
        .canonicalize()
        .with_context(|| format!("Local start path does not exist or is inaccessible: {}", path.display()))?;
    if !canonical.is_dir() {
        bail!("Local start path must be a directory: {}", canonical.display());
    }
    Ok(canonical)
}

fn validate_agent_id(agent: &str) -> Result<()> {
    if agent.len() != 4 || !agent.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()) {
        bail!("Agent ID must be exactly four lowercase ASCII letters/digits, for example k7m2");
    }
    Ok(())
}

fn ensure_local_runtime_host() -> Result<()> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        bail!(
            "Zodex Local runtime actions are macOS-only; `zodex local status`, `zodex local config`, and help remain available on this host"
        )
    }
}

#[cfg(test)]
mod local_cli_tests {
    use std::env;
    use std::io::{Seek as _, Write as _};
    #[cfg(unix)]
    use std::os::fd::AsRawFd as _;

    use super::{
        read_runtime_key_from_fd, resolve_local_setup_inputs, resolve_local_start_directory,
        trim_one_line_ending, validate_agent_id,
    };

    #[test]
    fn agent_id_validation_matches_public_contract() {
        for valid in ["k7m2", "0000", "abcd", "z9z9"] {
            validate_agent_id(valid).unwrap();
        }
        for invalid in ["abc", "abcde", "AB12", "a-b1", "åbc1"] {
            assert!(validate_agent_id(invalid).is_err());
        }
    }

    #[test]
    fn start_directory_must_exist_and_be_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_local_start_directory(Some(dir.path())).unwrap(),
            dir.path().canonicalize().unwrap()
        );
        let file = dir.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert!(resolve_local_start_directory(Some(&file)).is_err());
        assert!(resolve_local_start_directory(Some(&dir.path().join("missing"))).is_err());
    }

    #[test]
    fn setup_secret_line_endings_are_trimmed_once_only() {
        assert_eq!(trim_one_line_ending("secret\n".to_string()), "secret");
        assert_eq!(trim_one_line_ending("secret\r\n".to_string()), "secret");
        assert_eq!(trim_one_line_ending("secret\n\n".to_string()), "secret\n");
    }

    #[test]
    fn setup_env_input_is_scriptable_without_secret_argv() {
        let variable = format!("ZODEX_LOCAL_TEST_KEY_{}", std::process::id());
        // SAFETY: this unit test uses a process-unique variable and does not run
        // concurrent code that reads or mutates it.
        unsafe { env::set_var(&variable, "fixture-runtime-key\n") };
        let result = resolve_local_setup_inputs(
            Some("tunnel_0123456789abcdef0123456789abcdef".to_string()),
            false,
            Some(&variable),
            None,
        )
        .unwrap();
        // SAFETY: matching cleanup for the process-unique test variable above.
        unsafe { env::remove_var(&variable) };
        assert_eq!(result.0, "tunnel_0123456789abcdef0123456789abcdef");
        assert_eq!(result.1.expose(), "fixture-runtime-key");
    }

    #[cfg(unix)]
    #[test]
    fn setup_fd_input_marks_original_descriptor_close_on_exec() {
        use nix::fcntl::{FcntlArg, FdFlag, fcntl};

        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"fd-runtime-key\n").unwrap();
        file.rewind().unwrap();
        let fd = u32::try_from(file.as_raw_fd()).unwrap();

        let value = read_runtime_key_from_fd(fd).unwrap();
        assert_eq!(trim_one_line_ending(value), "fd-runtime-key");
        let flags = fcntl(&file, FcntlArg::F_GETFD).unwrap();
        assert!(FdFlag::from_bits_truncate(flags).contains(FdFlag::FD_CLOEXEC));
    }
}
