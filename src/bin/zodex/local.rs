use zodex::local::{
    LocalConfig, LocalPaths, LocalStatusDocument, LocalStatusState, ensure_offline_mutation,
    parse_human_duration,
};

#[derive(Debug, Subcommand)]
#[command(after_help = "Agent inspection examples:\n  zodex local status --json\n  zodex local history --last 20\n  zodex local history --since 30min\n  zodex local watch --agent k7m2")]
enum LocalCommand {
    /// Provision Local configuration, credentials, and the managed tunnel client.
    Setup,
    /// Start the one Mac-wide Local runtime from a repository or workspace.
    #[command(after_help = "Examples:\n  cd ~/code/amxv/zodex && zodex local start\n  zodex local start ~/code/amxv/zodex --ttl 4h\n\nPATH is startup guidance only. Every exec_command/apply_patch still supplies an explicit absolute workdir.")]
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
    #[command(after_help = "Examples:\n  zodex local history --last 20\n  zodex local history --since 30min\n  zodex local history --agent k7m2\n  zodex local history --format markdown\n  zodex local history clear --yes")]
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
    /// Stop Local and normally spawned processes across all Agents.
    Stop,
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

fn handle_local_command(command: LocalCommand) -> Result<()> {
    let paths = LocalPaths::discover()?;
    match command {
        LocalCommand::Setup => {
            ensure_local_runtime_host()?;
            bail!("`zodex local setup` provisioning is not available until the managed-setup phase")
        }
        LocalCommand::Start { path, ttl } => {
            let start_dir = resolve_local_start_directory(path.as_deref())?;
            if let Some(ttl) = ttl.as_deref() {
                parse_human_duration(ttl)
                    .with_context(|| format!("invalid Local service TTL `{ttl}`"))?;
            }
            ensure_local_runtime_host()?;
            bail!(
                "`zodex local start` lifecycle is not available yet (validated start directory: {})",
                start_dir.display()
            )
        }
        LocalCommand::Status { json } => print_local_status(&paths, json),
        LocalCommand::Watch { agent, all } => {
            if let Some(agent) = agent.as_deref() {
                validate_agent_id(agent)?;
            }
            let _ = all;
            ensure_local_runtime_host()?;
            bail!("`zodex local watch` is not available until the observability/TUI phases")
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
            if let Some(since) = since.as_deref() {
                parse_human_duration(since)
                    .with_context(|| format!("invalid history duration `{since}`"))?;
            }
            if let Some(agent) = agent.as_deref() {
                validate_agent_id(agent)?;
            }
            if let Some(workdir) = workdir.as_deref()
                && !workdir.is_absolute()
            {
                bail!("history --workdir must be an absolute path");
            }
            if !matches!(format.as_str(), "markdown" | "json") {
                bail!("history --format must be `markdown` or `json`");
            }
            let _ = (last, id, raw);
            if let Some(LocalHistoryCommand::Clear { yes }) = command {
                ensure_offline_mutation(&paths, "clear Local history")?;
                let _ = yes;
                bail!("Local history storage is not available until the durable-history phase")
            }
            bail!("Local history storage is not available until the durable-history phase")
        }
        LocalCommand::Config { command } => handle_local_config(&paths, command),
        LocalCommand::Stop => {
            ensure_local_runtime_host()?;
            bail!("`zodex local stop` lifecycle is not available until the launchd lifecycle phase")
        }
    }
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
    println!("Discovery: {}", status.discovery_path.display());
    if status.discovery.is_none() {
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
    use super::{resolve_local_start_directory, validate_agent_id};

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
}
