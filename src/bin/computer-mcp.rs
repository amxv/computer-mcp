use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::net::IpAddr;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use computer_mcp::config::{Config, DEFAULT_CONFIG_PATH};
use computer_mcp::install_rustls_crypto_provider;
use computer_mcp::publisher::{build_publish_request, detect_repo_root, submit_publish_request};
use computer_mcp::redaction::redact_api_key_query_params;
#[cfg(unix)]
use nix::errno::Errno;
#[cfg(unix)]
use nix::sys::signal::{Signal, kill};
#[cfg(unix)]
use nix::unistd::{Group, Pid, Uid, User, chown, setsid};
use rand::distr::{Alphanumeric, SampleString};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};

const SERVICE_NAME: &str = "computer-mcpd.service";
const SYSTEMD_UNIT_PATH: &str = "/etc/systemd/system/computer-mcpd.service";
const STATE_DIR: &str = "/var/lib/computer-mcp";
const TLS_DIR: &str = "/var/lib/computer-mcp/tls";
const LETSENCRYPT_LIVE_DIR: &str = "/etc/letsencrypt/live";
const DEFAULT_LOG_LINES: &str = "200";
const STATUS_HOST_HINT_FALLBACK: &str = "<host>";
const TLS_MODE_LETSENCRYPT_IP: &str = "letsencrypt_ip";
const TLS_MODE_SELF_SIGNED: &str = "self_signed";
const PROCESS_RUNTIME_DIRNAME: &str = "run";
const PROCESS_LOG_DIRNAME: &str = "logs";
const PROCESS_PID_FILENAME: &str = "computer-mcpd.pid";
const PROCESS_LOG_FILENAME: &str = "computer-mcpd.log";
const PUBLISHER_PROCESS_SUBDIR: &str = "publisher";
const PUBLISHER_SERVICE_LABEL: &str = "computer-mcp-prd";
const PUBLISHER_PROCESS_PID_FILENAME: &str = "computer-mcp-prd.pid";
const PUBLISHER_PROCESS_LOG_FILENAME: &str = "computer-mcp-prd.log";
const PROCESS_START_STABILIZE_MS: u64 = 300;
const PROCESS_STOP_TIMEOUT_MS: u64 = 5_000;
const PROCESS_STOP_POLL_MS: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceManager {
    Systemd,
    Process,
}

#[derive(Debug, Parser)]
#[command(name = "computer-mcp")]
#[command(about = "Computer MCP CLI")]
#[command(version)]
struct Cli {
    #[arg(long, default_value = DEFAULT_CONFIG_PATH)]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install,
    Upgrade {
        #[arg(long, default_value = "latest")]
        version: String,
    },
    Start,
    Stop,
    Restart,
    Status,
    Logs,
    SetKey {
        value: String,
    },
    RotateKey,
    ShowUrl {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    Tls {
        #[command(subcommand)]
        command: TlsCommand,
    },
    PublishPr {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        body_file: Option<String>,
        #[arg(long)]
        base: Option<String>,
        #[arg(long, default_value_t = false)]
        draft: bool,
    },
    Publisher {
        #[command(subcommand)]
        command: PublisherCommand,
    },
}

#[derive(Debug, Subcommand)]
enum TlsCommand {
    Setup,
}

#[derive(Debug, Subcommand)]
enum PublisherCommand {
    Start,
    Stop,
    Status,
    Logs,
}

#[tokio::main]
async fn main() -> Result<()> {
    install_rustls_crypto_provider();

    let cli = Cli::parse();
    let config_path = PathBuf::from(&cli.config);

    match cli.command {
        Commands::Install => {
            install(&config_path)?;
        }
        Commands::Upgrade { version } => {
            ensure_linux()?;
            upgrade(&config_path, &version)?;
        }
        Commands::Start => {
            ensure_linux()?;
            start_stack(&config_path)?;
        }
        Commands::Stop => {
            ensure_linux()?;
            let config = Config::load(Some(Path::new(&config_path)))?;
            stop_stack(&config)?;
        }
        Commands::Restart => {
            ensure_linux()?;
            restart_stack(&config_path)?;
        }
        Commands::Status => {
            ensure_linux()?;
            let config = Config::load(Some(Path::new(&config_path)))?;
            print_stack_status_summary(&config)?;
        }
        Commands::Logs => {
            ensure_linux()?;
            let config = Config::load(Some(Path::new(&config_path)))?;
            match detect_service_manager() {
                ServiceManager::Systemd => {
                    let logs = run_journalctl(&build_journalctl_args())?;
                    if logs.is_empty() {
                        println!("no recent logs found for {SERVICE_NAME}");
                    } else {
                        print!("{}", redact_api_key_query_params(&logs));
                    }
                }
                ServiceManager::Process => {
                    let logs =
                        read_process_logs(&config, DEFAULT_LOG_LINES.parse().unwrap_or(200))?;
                    if logs.is_empty() {
                        println!(
                            "no recent logs found for {}",
                            process_log_path(&config).display()
                        );
                    } else {
                        print!("{}", redact_api_key_query_params(&logs));
                    }
                }
            }
        }
        Commands::SetKey { value } => {
            let mut config = Config::load(Some(Path::new(&config_path)))?;
            config.api_key = value;
            config.save(&config_path)?;
            ensure_shared_group_permissions(&config, &config_path)?;
            println!("updated API key in {}", config_path.display());
        }
        Commands::RotateKey => {
            let mut config = Config::load(Some(Path::new(&config_path)))?;
            let mut rng = rand::rng();
            config.api_key = Alphanumeric.sample_string(&mut rng, 48);
            config.save(&config_path)?;
            ensure_shared_group_permissions(&config, &config_path)?;
            println!("rotated API key in {}", config_path.display());
        }
        Commands::ShowUrl { host } => {
            let config = Config::load(Some(Path::new(&config_path)))?;
            let raw_url = format!("https://{host}/mcp?key={}", config.api_key);
            println!(
                "{} (key redacted in CLI output)",
                redact_api_key_query_params(&raw_url)
            );
        }
        Commands::Tls { command } => match command {
            TlsCommand::Setup => tls_setup(&config_path)?,
        },
        Commands::PublishPr {
            repo,
            title,
            body,
            body_file,
            base,
            draft,
        } => {
            ensure_linux()?;
            let config = Config::load(Some(Path::new(&config_path)))?;
            let body = resolve_pr_body(body, body_file.as_deref())?;
            let cwd = std::env::current_dir().context("failed to resolve current directory")?;
            let repo_root = detect_repo_root(&cwd)?;
            let request =
                build_publish_request(&config, repo, base, title, body, draft, &repo_root)?;
            let response =
                submit_publish_request(Path::new(&config.publisher_socket_path), &request).await?;
            println!("pr-url: {}", response.pr_url);
            println!("branch: {}", response.branch);
            println!("pull-number: {}", response.pull_number);
        }
        Commands::Publisher { command } => {
            ensure_linux()?;
            let config = Config::load(Some(Path::new(&config_path)))?;
            match command {
                PublisherCommand::Start => start_publisher_process_mode(&config, &config_path)?,
                PublisherCommand::Stop => stop_publisher_process_mode(&config)?,
                PublisherCommand::Status => print_publisher_status_summary(&config),
                PublisherCommand::Logs => {
                    let logs =
                        read_publisher_logs(&config, DEFAULT_LOG_LINES.parse().unwrap_or(200))?;
                    if logs.is_empty() {
                        println!(
                            "no recent logs found for {}",
                            publisher_process_log_path(&config).display()
                        );
                    } else {
                        print!("{logs}");
                    }
                }
            }
        }
    }

    Ok(())
}

fn resolve_pr_body(body: Option<String>, body_file: Option<&str>) -> Result<String> {
    match (body, body_file) {
        (Some(_), Some(_)) => bail!("--body and --body-file are mutually exclusive"),
        (Some(body), None) => Ok(body),
        (None, Some(path)) => {
            fs::read_to_string(path).with_context(|| format!("failed to read PR body file {path}"))
        }
        (None, None) => Ok(String::new()),
    }
}

fn start_stack(config_path: &Path) -> Result<()> {
    let mut config = Config::load(Some(config_path))?;
    ensure_stack_config_ready(&config)?;

    if !tls_artifacts_exist(&config) {
        println!("TLS artifacts missing; creating them automatically");
        config = provision_tls_artifacts(config_path, false)?;
    }

    start_publisher_process_mode(&config, config_path)?;
    start_main_service(&config, config_path)?;
    print_stack_ready_summary(&config);
    Ok(())
}

fn stop_stack(config: &Config) -> Result<()> {
    stop_main_service(config)?;
    stop_publisher_process_mode(config)?;
    Ok(())
}

fn restart_stack(config_path: &Path) -> Result<()> {
    let mut config = Config::load(Some(config_path))?;
    ensure_stack_config_ready(&config)?;

    if !tls_artifacts_exist(&config) {
        println!("TLS artifacts missing; creating them automatically");
        config = provision_tls_artifacts(config_path, false)?;
    }

    stop_main_service(&config)?;
    stop_publisher_process_mode(&config)?;
    start_publisher_process_mode(&config, config_path)?;
    start_main_service(&config, config_path)?;
    print_stack_ready_summary(&config);
    Ok(())
}

fn start_main_service(config: &Config, config_path: &Path) -> Result<()> {
    match detect_service_manager() {
        ServiceManager::Systemd => {
            run_systemctl(&build_systemctl_args(SystemctlAction::Start))?;
            println!("started {SERVICE_NAME}");
            Ok(())
        }
        ServiceManager::Process => start_process_mode(config, config_path),
    }
}

fn stop_main_service(config: &Config) -> Result<()> {
    match detect_service_manager() {
        ServiceManager::Systemd => {
            run_systemctl(&build_systemctl_args(SystemctlAction::Stop))?;
            println!("stopped {SERVICE_NAME}");
            Ok(())
        }
        ServiceManager::Process => stop_process_mode(config),
    }
}

fn print_stack_ready_summary(config: &Config) {
    let host_hint = status_host_hint(&config.bind_host, detect_public_ip());
    let url_hint =
        redact_api_key_query_params(&format!("https://{host_hint}/mcp?key={}", config.api_key));
    println!("stack-ready: {SERVICE_NAME} + {PUBLISHER_SERVICE_LABEL}");
    println!("url-hint: {url_hint}");
    if let Some(port) = config.http_bind_port {
        println!("http-proxy-listen: {}:{port}", config.bind_host);
    }
}

fn install(config_path: &Path) -> Result<()> {
    ensure_linux()?;
    create_required_dirs(config_path)?;
    ensure_config_exists(config_path)?;
    let config = Config::load(Some(config_path))?;

    match detect_service_manager() {
        ServiceManager::Systemd => {
            let daemon_path = resolve_daemon_binary_path()?;
            let unit_content = render_systemd_unit(&daemon_path, config_path);
            let unit_changed = write_if_changed(Path::new(SYSTEMD_UNIT_PATH), &unit_content)?;
            if unit_changed {
                println!("wrote unit file at {SYSTEMD_UNIT_PATH}");
            } else {
                println!("unit file already up to date at {SYSTEMD_UNIT_PATH}");
            }

            run_systemctl(&build_systemctl_args(SystemctlAction::DaemonReload))?;
            run_systemctl(&build_systemctl_args(SystemctlAction::Enable))?;
            println!("enabled {SERVICE_NAME} for boot persistence");
        }
        ServiceManager::Process => {
            ensure_process_mode_dirs(&config)?;
            ensure_publisher_process_dirs(&config)?;
            println!(
                "systemd not detected; configured process mode for container-style environments"
            );
            println!(
                "process mode files: pid={}, log={}",
                process_pid_path(&config).display(),
                process_log_path(&config).display()
            );
            println!(
                "publisher process mode files: pid={}, log={}, socket={}",
                publisher_process_pid_path(&config).display(),
                publisher_process_log_path(&config).display(),
                config.publisher_socket_path
            );
        }
    }
    Ok(())
}

fn upgrade(config_path: &Path, version: &str) -> Result<()> {
    let config = Config::load(Some(config_path))?;

    let install_args = build_upgrade_shell_args(version, &config);
    run_shell_script(&install_args)?;
    restart_stack(config_path)?;
    Ok(())
}

fn build_upgrade_shell_args(version: &str, config: &Config) -> Vec<String> {
    let mut script = format!(
        "set -euo pipefail\nexport COMPUTER_MCP_VERSION={}\n",
        shell_escape_single_quotes(version)
    );

    if let Some(port) = config.http_bind_port {
        script.push_str(&format!("export COMPUTER_MCP_HTTP_BIND_PORT={port}\n"));
    }

    script.push_str("curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | bash");
    vec!["-lc".to_string(), script]
}

fn shell_escape_single_quotes(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn run_shell_script(args: &[String]) -> Result<String> {
    run_command_capture("bash", args)
}

fn tls_setup(config_path: &Path) -> Result<()> {
    provision_tls_artifacts(config_path, true)?;
    Ok(())
}

fn provision_tls_artifacts(config_path: &Path, restart_after: bool) -> Result<Config> {
    ensure_linux()?;
    let mut config = Config::load(Some(config_path))?;
    ensure_tls_dirs_for_config(&config)?;

    let san_ip = select_tls_san_ip(&config.bind_host, detect_public_ip());
    println!("tls setup target IP SAN: {san_ip}");

    match try_setup_letsencrypt_ip(&config, san_ip) {
        Ok(()) => {
            config.tls_mode = TLS_MODE_LETSENCRYPT_IP.to_string();
            println!("acquired Let's Encrypt IP certificate");
        }
        Err(err) => {
            eprintln!(
                "warning: Let's Encrypt IP certificate setup failed, falling back to self-signed: {err}"
            );
            generate_self_signed_certificate(&config, san_ip)?;
            config.tls_mode = TLS_MODE_SELF_SIGNED.to_string();
            println!(
                "generated self-signed certificate fallback at {} and {}",
                config.tls_cert_path, config.tls_key_path
            );
        }
    }

    config.save(config_path)?;
    ensure_shared_group_permissions(&config, config_path)?;
    println!("updated TLS settings in {}", config_path.display());
    if restart_after {
        restart_service_after_tls_setup(&config, config_path);
    }
    Ok(config)
}

fn ensure_linux() -> Result<()> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        bail!("computer-mcp CLI service management is Linux-only");
    }
}

fn detect_service_manager() -> ServiceManager {
    if !command_exists("systemctl") {
        return ServiceManager::Process;
    }

    match fs::read_to_string("/proc/1/comm") {
        Ok(pid1_comm) => service_manager_from_pid1(pid1_comm.trim()),
        Err(_) => ServiceManager::Process,
    }
}

fn service_manager_from_pid1(pid1_comm: &str) -> ServiceManager {
    if pid1_comm == "systemd" {
        ServiceManager::Systemd
    } else {
        ServiceManager::Process
    }
}

fn command_exists(program: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {program} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
fn current_euid_is_root() -> bool {
    Uid::effective().is_root()
}

#[cfg(not(unix))]
fn current_euid_is_root() -> bool {
    false
}

#[cfg(unix)]
fn lookup_user(name: &str) -> Result<User> {
    User::from_name(name)
        .context("failed to query local user database")?
        .ok_or_else(|| anyhow!("local user not found: {name}"))
}

#[cfg(unix)]
fn lookup_group(name: &str) -> Result<Group> {
    Group::from_name(name)
        .context("failed to query local group database")?
        .ok_or_else(|| anyhow!("local group not found: {name}"))
}

#[cfg(unix)]
fn chown_path_to_user(path: &Path, user: &User) -> Result<()> {
    chown(path, Some(user.uid), Some(user.gid))
        .with_context(|| format!("failed to chown {} to {}", path.display(), user.name))
}

#[cfg(unix)]
fn chown_path_to_group(path: &Path, group: &Group) -> Result<()> {
    chown(path, None, Some(group.gid))
        .with_context(|| format!("failed to chgrp {} to {}", path.display(), group.name))
}

#[cfg(unix)]
fn ensure_runuser_available() -> Result<()> {
    if command_exists("runuser") {
        Ok(())
    } else {
        bail!("`runuser` is required to launch daemons under separate users")
    }
}

fn create_required_dirs(config_path: &Path) -> Result<()> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    fs::create_dir_all(STATE_DIR).with_context(|| format!("failed to create {STATE_DIR}"))?;
    fs::create_dir_all(TLS_DIR).with_context(|| format!("failed to create {TLS_DIR}"))?;
    Ok(())
}

fn ensure_tls_dirs_for_config(config: &Config) -> Result<()> {
    if let Some(parent) = Path::new(&config.tls_cert_path).parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create TLS cert directory {}", parent.display()))?;
    }
    if let Some(parent) = Path::new(&config.tls_key_path).parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create TLS key directory {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_shared_group_permissions(config: &Config, config_path: &Path) -> Result<()> {
    if !current_euid_is_root() {
        return Ok(());
    }

    let Ok(group) = lookup_group(&config.service_group) else {
        return Ok(());
    };

    if config_path.exists() {
        chown_path_to_group(config_path, &group)?;
        set_file_mode(config_path, 0o640)?;
    }

    let cert_path = Path::new(&config.tls_cert_path);
    if cert_path.exists() {
        chown_path_to_group(cert_path, &group)?;
        set_file_mode(cert_path, 0o644)?;
    }

    let key_path = Path::new(&config.tls_key_path);
    if key_path.exists() {
        chown_path_to_group(key_path, &group)?;
        set_file_mode(key_path, 0o640)?;
    }

    Ok(())
}

#[cfg(not(unix))]
fn ensure_shared_group_permissions(_config: &Config, _config_path: &Path) -> Result<()> {
    Ok(())
}

fn ensure_config_exists(config_path: &Path) -> Result<()> {
    if config_path.exists() {
        return Ok(());
    }

    let config = Config::default();
    config.save(config_path)?;
    ensure_shared_group_permissions(&config, config_path)?;
    println!("created default config at {}", config_path.display());
    Ok(())
}

fn ensure_stack_config_ready(config: &Config) -> Result<()> {
    ensure_reader_ready_for_start(config)?;
    ensure_publisher_ready_for_start(config)?;
    ensure_http_listener_ready_for_start(config)?;
    Ok(())
}

fn ensure_http_listener_ready_for_start(config: &Config) -> Result<()> {
    if let Some(port) = config.http_bind_port {
        if port == config.bind_port {
            bail!("http_bind_port must differ from bind_port");
        }
    }

    Ok(())
}

fn ensure_reader_ready_for_start(config: &Config) -> Result<()> {
    let Some(app_id) = config.reader_app_id else {
        bail!("reader_app_id must be configured before start");
    };
    if app_id == 0 {
        bail!("reader_app_id must be non-zero");
    }

    let Some(installation_id) = config.reader_installation_id else {
        bail!("reader_installation_id must be configured before start");
    };
    if installation_id == 0 {
        bail!("reader_installation_id must be non-zero");
    }

    if config.reader_private_key_path.trim().is_empty() {
        bail!("reader_private_key_path must be configured");
    }
    if !Path::new(&config.reader_private_key_path).exists() {
        bail!(
            "reader private key file not found: {}",
            config.reader_private_key_path
        );
    }

    Ok(())
}

fn ensure_publisher_ready_for_start(config: &Config) -> Result<()> {
    let Some(app_id) = config.publisher_app_id else {
        bail!("publisher_app_id must be configured before start");
    };
    if app_id == 0 {
        bail!("publisher_app_id must be non-zero");
    }

    if config.publisher_private_key_path.trim().is_empty() {
        bail!("publisher_private_key_path must be configured");
    }
    if !Path::new(&config.publisher_private_key_path).exists() {
        bail!(
            "publisher private key file not found: {}",
            config.publisher_private_key_path
        );
    }
    if config.publisher_targets.is_empty() {
        bail!("publisher_targets must contain at least one allowed repo target");
    }

    for target in &config.publisher_targets {
        if target.id.trim().is_empty() || target.repo.trim().is_empty() {
            bail!("publisher target entries require both id and repo");
        }
        if target.installation_id == 0 {
            bail!("publisher target {} must define installation_id", target.id);
        }
    }

    Ok(())
}

fn resolve_daemon_binary_path() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("COMPUTER_MCPD_PATH") {
        let path = PathBuf::from(&override_path);
        if path.exists() {
            return Ok(path);
        }
        bail!("COMPUTER_MCPD_PATH does not exist: {override_path}");
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    if let Some(parent) = current_exe.parent() {
        candidates.push(parent.join("computer-mcpd"));
    }
    candidates.push(PathBuf::from("/usr/local/bin/computer-mcpd"));
    candidates.push(PathBuf::from("/usr/bin/computer-mcpd"));

    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| anyhow!("failed to locate computer-mcpd binary"))
}

fn resolve_publisher_daemon_binary_path() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("COMPUTER_MCP_PRD_PATH") {
        let path = PathBuf::from(&override_path);
        if path.exists() {
            return Ok(path);
        }
        bail!("COMPUTER_MCP_PRD_PATH does not exist: {override_path}");
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    if let Some(parent) = current_exe.parent() {
        candidates.push(parent.join(PUBLISHER_SERVICE_LABEL));
    }
    candidates.push(PathBuf::from(format!(
        "/usr/local/bin/{PUBLISHER_SERVICE_LABEL}"
    )));
    candidates.push(PathBuf::from(format!("/usr/bin/{PUBLISHER_SERVICE_LABEL}")));

    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| anyhow!("failed to locate {PUBLISHER_SERVICE_LABEL} binary"))
}

fn state_root_for_config(config: &Config) -> PathBuf {
    let cert_path = Path::new(&config.tls_cert_path);
    if let Some(parent) = cert_path.parent().and_then(Path::parent) {
        return parent.to_path_buf();
    }
    PathBuf::from(STATE_DIR)
}

fn process_runtime_dir(config: &Config) -> PathBuf {
    state_root_for_config(config).join(PROCESS_RUNTIME_DIRNAME)
}

fn process_log_dir(config: &Config) -> PathBuf {
    state_root_for_config(config).join(PROCESS_LOG_DIRNAME)
}

fn process_pid_path(config: &Config) -> PathBuf {
    process_runtime_dir(config).join(PROCESS_PID_FILENAME)
}

fn process_log_path(config: &Config) -> PathBuf {
    process_log_dir(config).join(PROCESS_LOG_FILENAME)
}

fn publisher_process_root(config: &Config) -> PathBuf {
    state_root_for_config(config).join(PUBLISHER_PROCESS_SUBDIR)
}

fn publisher_process_runtime_dir(config: &Config) -> PathBuf {
    publisher_process_root(config).join(PROCESS_RUNTIME_DIRNAME)
}

fn publisher_process_log_dir(config: &Config) -> PathBuf {
    publisher_process_root(config).join(PROCESS_LOG_DIRNAME)
}

fn publisher_process_pid_path(config: &Config) -> PathBuf {
    publisher_process_runtime_dir(config).join(PUBLISHER_PROCESS_PID_FILENAME)
}

fn publisher_process_log_path(config: &Config) -> PathBuf {
    publisher_process_log_dir(config).join(PUBLISHER_PROCESS_LOG_FILENAME)
}

fn ensure_process_mode_dirs(config: &Config) -> Result<()> {
    fs::create_dir_all(process_runtime_dir(config))
        .with_context(|| format!("failed to create {}", process_runtime_dir(config).display()))?;
    fs::create_dir_all(process_log_dir(config))
        .with_context(|| format!("failed to create {}", process_log_dir(config).display()))?;
    Ok(())
}

fn ensure_publisher_process_dirs(config: &Config) -> Result<()> {
    fs::create_dir_all(publisher_process_runtime_dir(config)).with_context(|| {
        format!(
            "failed to create {}",
            publisher_process_runtime_dir(config).display()
        )
    })?;
    fs::create_dir_all(publisher_process_log_dir(config)).with_context(|| {
        format!(
            "failed to create {}",
            publisher_process_log_dir(config).display()
        )
    })?;
    if let Some(parent) = Path::new(&config.publisher_socket_path).parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn prepare_agent_process_ownership(config: &Config) -> Result<()> {
    if !current_euid_is_root() {
        return Ok(());
    }

    let user = lookup_user(&config.agent_user)?;
    chown_path_to_user(&process_runtime_dir(config), &user)?;
    chown_path_to_user(&process_log_dir(config), &user)?;
    Ok(())
}

#[cfg(not(unix))]
fn prepare_agent_process_ownership(_config: &Config) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn prepare_publisher_process_ownership(config: &Config) -> Result<()> {
    if !current_euid_is_root() {
        return Ok(());
    }

    let user = lookup_user(&config.publisher_user)?;
    chown_path_to_user(&publisher_process_root(config), &user)?;
    chown_path_to_user(&publisher_process_runtime_dir(config), &user)?;
    chown_path_to_user(&publisher_process_log_dir(config), &user)?;
    if let Some(parent) = Path::new(&config.publisher_socket_path).parent() {
        chown_path_to_user(parent, &user)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn prepare_publisher_process_ownership(_config: &Config) -> Result<()> {
    Ok(())
}

fn read_process_pid(config: &Config) -> Result<Option<i32>> {
    let pid_path = process_pid_path(config);
    if !pid_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&pid_path)
        .with_context(|| format!("failed to read {}", pid_path.display()))?;
    let pid = raw
        .trim()
        .parse::<i32>()
        .with_context(|| format!("invalid pid in {}", pid_path.display()))?;
    Ok(Some(pid))
}

fn read_publisher_pid(config: &Config) -> Result<Option<i32>> {
    let pid_path = publisher_process_pid_path(config);
    if !pid_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&pid_path)
        .with_context(|| format!("failed to read {}", pid_path.display()))?;
    let pid = raw
        .trim()
        .parse::<i32>()
        .with_context(|| format!("invalid pid in {}", pid_path.display()))?;
    Ok(Some(pid))
}

fn pid_is_running(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn daemon_launch_command(
    binary_path: &Path,
    config_path: &Path,
    run_user: &str,
) -> Result<Command> {
    #[cfg(unix)]
    if current_euid_is_root() {
        ensure_runuser_available()?;
        let mut command = Command::new("runuser");
        command
            .arg("-u")
            .arg(run_user)
            .arg("--")
            .arg(binary_path)
            .arg("--config")
            .arg(config_path);
        return Ok(command);
    }

    let mut command = Command::new(binary_path);
    command.arg("--config").arg(config_path);
    Ok(command)
}

fn remove_pid_file_if_present(config: &Config) -> Result<()> {
    let pid_path = process_pid_path(config);
    if pid_path.exists() {
        fs::remove_file(&pid_path)
            .with_context(|| format!("failed to remove {}", pid_path.display()))?;
    }
    Ok(())
}

fn remove_publisher_pid_file_if_present(config: &Config) -> Result<()> {
    let pid_path = publisher_process_pid_path(config);
    if pid_path.exists() {
        fs::remove_file(&pid_path)
            .with_context(|| format!("failed to remove {}", pid_path.display()))?;
    }
    Ok(())
}

fn start_process_mode(config: &Config, config_path: &Path) -> Result<()> {
    ensure_process_mode_dirs(config)?;
    prepare_agent_process_ownership(config)?;

    if let Some(pid) = read_process_pid(config)? {
        if pid_is_running(pid) {
            println!("{SERVICE_NAME} already running in process mode (pid {pid})");
            println!("log file: {}", process_log_path(config).display());
            return Ok(());
        }
        remove_pid_file_if_present(config)?;
    }

    let daemon_path = resolve_daemon_binary_path()?;
    let log_path = process_log_path(config);
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("failed to clone {}", log_path.display()))?;

    let mut command = daemon_launch_command(&daemon_path, config_path, &config.agent_user)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .current_dir("/");

    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            setsid().map_err(|e| io::Error::other(e.to_string()))?;
            Ok(())
        });
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn {}", daemon_path.display()))?;
    let pid = child.id() as i32;

    thread::sleep(Duration::from_millis(PROCESS_START_STABILIZE_MS));
    if let Some(status) = child.try_wait().context("failed to inspect child status")? {
        let recent_logs = read_process_logs(config, 50).unwrap_or_default();
        let details = if recent_logs.trim().is_empty() {
            "no recent process log output".to_string()
        } else {
            format!(
                "recent log output:\n{}",
                redact_api_key_query_params(&recent_logs)
            )
        };
        bail!("{SERVICE_NAME} exited immediately in process mode (status: {status})\n{details}");
    }

    fs::write(process_pid_path(config), format!("{pid}\n"))
        .with_context(|| format!("failed to write {}", process_pid_path(config).display()))?;
    println!("started {SERVICE_NAME} in process mode (pid {pid})");
    println!("log file: {}", log_path.display());
    Ok(())
}

fn stop_process_mode(config: &Config) -> Result<()> {
    let Some(pid) = read_process_pid(config)? else {
        println!("{SERVICE_NAME} is not running in process mode");
        return Ok(());
    };

    if !pid_is_running(pid) {
        remove_pid_file_if_present(config)?;
        println!("removed stale pid file for {SERVICE_NAME} (pid {pid})");
        return Ok(());
    }

    send_signal_if_running(pid, Signal::SIGTERM)?;
    let deadline = Instant::now() + Duration::from_millis(PROCESS_STOP_TIMEOUT_MS);
    while pid_is_running(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(PROCESS_STOP_POLL_MS));
    }

    if pid_is_running(pid) {
        send_signal_if_running(pid, Signal::SIGKILL)?;
        let kill_deadline = Instant::now() + Duration::from_millis(PROCESS_STOP_TIMEOUT_MS);
        while pid_is_running(pid) && Instant::now() < kill_deadline {
            thread::sleep(Duration::from_millis(PROCESS_STOP_POLL_MS));
        }
    }

    remove_pid_file_if_present(config)?;
    println!("stopped {SERVICE_NAME} in process mode");
    Ok(())
}

fn read_process_logs(config: &Config, max_lines: usize) -> Result<String> {
    let log_path = process_log_path(config);
    if !log_path.exists() {
        return Ok(String::new());
    }

    read_tail_lines(&log_path, max_lines)
}

fn start_publisher_process_mode(config: &Config, config_path: &Path) -> Result<()> {
    ensure_publisher_process_dirs(config)?;
    prepare_publisher_process_ownership(config)?;

    if let Some(pid) = read_publisher_pid(config)? {
        if pid_is_running(pid) {
            println!("{PUBLISHER_SERVICE_LABEL} already running in process mode (pid {pid})");
            println!("log file: {}", publisher_process_log_path(config).display());
            return Ok(());
        }
        remove_publisher_pid_file_if_present(config)?;
    }

    let daemon_path = resolve_publisher_daemon_binary_path()?;
    let log_path = publisher_process_log_path(config);
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("failed to clone {}", log_path.display()))?;

    let mut command = daemon_launch_command(&daemon_path, config_path, &config.publisher_user)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .current_dir("/");

    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            setsid().map_err(|e| io::Error::other(e.to_string()))?;
            Ok(())
        });
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn {}", daemon_path.display()))?;
    let pid = child.id() as i32;

    thread::sleep(Duration::from_millis(PROCESS_START_STABILIZE_MS));
    if let Some(status) = child.try_wait().context("failed to inspect child status")? {
        let recent_logs = read_publisher_logs(config, 50).unwrap_or_default();
        let details = if recent_logs.trim().is_empty() {
            "no recent process log output".to_string()
        } else {
            format!("recent log output:\n{}", recent_logs)
        };
        bail!(
            "{PUBLISHER_SERVICE_LABEL} exited immediately in process mode (status: {status})\n{details}"
        );
    }

    fs::write(publisher_process_pid_path(config), format!("{pid}\n")).with_context(|| {
        format!(
            "failed to write {}",
            publisher_process_pid_path(config).display()
        )
    })?;
    println!("started {PUBLISHER_SERVICE_LABEL} in process mode (pid {pid})");
    println!("log file: {}", log_path.display());
    Ok(())
}

fn stop_publisher_process_mode(config: &Config) -> Result<()> {
    let Some(pid) = read_publisher_pid(config)? else {
        println!("{PUBLISHER_SERVICE_LABEL} is not running in process mode");
        return Ok(());
    };

    if !pid_is_running(pid) {
        remove_publisher_pid_file_if_present(config)?;
        println!("removed stale pid file for {PUBLISHER_SERVICE_LABEL} (pid {pid})");
        return Ok(());
    }

    send_signal_if_running(pid, Signal::SIGTERM)?;
    let deadline = Instant::now() + Duration::from_millis(PROCESS_STOP_TIMEOUT_MS);
    while pid_is_running(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(PROCESS_STOP_POLL_MS));
    }

    if pid_is_running(pid) {
        send_signal_if_running(pid, Signal::SIGKILL)?;
        let kill_deadline = Instant::now() + Duration::from_millis(PROCESS_STOP_TIMEOUT_MS);
        while pid_is_running(pid) && Instant::now() < kill_deadline {
            thread::sleep(Duration::from_millis(PROCESS_STOP_POLL_MS));
        }
    }

    remove_publisher_pid_file_if_present(config)?;
    println!("stopped {PUBLISHER_SERVICE_LABEL} in process mode");
    Ok(())
}

fn read_publisher_logs(config: &Config, max_lines: usize) -> Result<String> {
    let log_path = publisher_process_log_path(config);
    if !log_path.exists() {
        return Ok(String::new());
    }

    read_tail_lines(&log_path, max_lines)
}

fn read_tail_lines(path: &Path, max_lines: usize) -> Result<String> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    let mut result = lines[start..].join("\n");
    if content.ends_with('\n') && !result.is_empty() {
        result.push('\n');
    }
    Ok(result)
}

#[cfg(unix)]
fn send_signal_if_running(pid: i32, signal: Signal) -> Result<()> {
    match kill(Pid::from_raw(pid), signal) {
        Ok(_) | Err(Errno::ESRCH) => Ok(()),
        Err(err) => Err(anyhow!("failed to send {signal:?} to pid {pid}: {err}")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessModeState {
    Running(i32),
    Stale(i32),
    Stopped,
}

fn process_mode_state(config: &Config) -> Result<ProcessModeState> {
    match read_process_pid(config)? {
        Some(pid) if pid_is_running(pid) => Ok(ProcessModeState::Running(pid)),
        Some(pid) => Ok(ProcessModeState::Stale(pid)),
        None => Ok(ProcessModeState::Stopped),
    }
}

fn print_stack_status_summary(config: &Config) -> Result<()> {
    let main_lines = build_main_status_lines(config)?;
    for line in main_lines {
        println!("{line}");
    }

    println!();
    for line in build_publisher_status_lines(config, publisher_process_mode_state(config))? {
        println!("{line}");
    }

    println!();
    for line in build_reader_status_lines(config) {
        println!("{line}");
    }

    Ok(())
}

fn build_main_status_lines(config: &Config) -> Result<Vec<String>> {
    match detect_service_manager() {
        ServiceManager::Systemd => {
            let raw = run_systemctl(&build_systemctl_args(SystemctlAction::ShowStatus))?;
            Ok(build_status_summary_lines(&raw, config, detect_public_ip()))
        }
        ServiceManager::Process => {
            build_process_status_lines(config, detect_public_ip(), process_mode_state(config))
        }
    }
}

fn print_process_status_summary(config: &Config) {
    match build_process_status_lines(config, detect_public_ip(), process_mode_state(config)) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
        }
        Err(err) => eprintln!("warning: failed to build process mode status: {err}"),
    }
}

fn publisher_process_mode_state(config: &Config) -> Result<ProcessModeState> {
    match read_publisher_pid(config)? {
        Some(pid) if pid_is_running(pid) => Ok(ProcessModeState::Running(pid)),
        Some(pid) => Ok(ProcessModeState::Stale(pid)),
        None => Ok(ProcessModeState::Stopped),
    }
}

fn print_publisher_status_summary(config: &Config) {
    match build_publisher_status_lines(config, publisher_process_mode_state(config)) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
        }
        Err(err) => eprintln!("warning: failed to build publisher status: {err}"),
    }
}

fn build_reader_status_lines(config: &Config) -> Vec<String> {
    let app_id = config
        .reader_app_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "<unset>".to_string());
    let installation_id = config
        .reader_installation_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "<unset>".to_string());
    let ready = ensure_reader_ready_for_start(config).is_ok();

    let mut lines = vec![
        "service: computer-mcp-reader".to_string(),
        "service-mode: config-only".to_string(),
        format!("active: {}", if ready { "ready" } else { "not-ready" }),
        format!("reader-app-id: {app_id}"),
        format!("reader-installation-id: {installation_id}"),
        format!("reader-key: {}", config.reader_private_key_path),
    ];

    if config.reader_app_id.is_none() {
        lines.push("hint: set `reader_app_id` in config".to_string());
    }
    if config.reader_installation_id.is_none() {
        lines.push("hint: set `reader_installation_id` in config".to_string());
    }
    if !Path::new(&config.reader_private_key_path).exists() {
        lines.push("hint: place the reader private key at the configured path".to_string());
    }

    lines
}

fn build_process_status_lines(
    config: &Config,
    public_ip: Option<IpAddr>,
    state: Result<ProcessModeState>,
) -> Result<Vec<String>> {
    let state = state?;
    let host_hint = status_host_hint(&config.bind_host, public_ip);
    let url_hint =
        redact_api_key_query_params(&format!("https://{host_hint}/mcp?key={}", config.api_key));
    let health_hint = format!("https://{host_hint}/health");
    let active = match state {
        ProcessModeState::Running(_) => "active (running)",
        ProcessModeState::Stale(_) => "inactive (stale pid file)",
        ProcessModeState::Stopped => "inactive (dead)",
    };
    let exec_status = match state {
        ProcessModeState::Running(pid) => format!("running pid {pid}"),
        ProcessModeState::Stale(pid) => format!("stale pid file {pid}"),
        ProcessModeState::Stopped => "not running".to_string(),
    };

    let mut lines = vec![
        format!("service: {SERVICE_NAME}"),
        "service-mode: process".to_string(),
        format!("active: {active}"),
        "enabled: n/a (process mode)".to_string(),
        "unit-file: n/a (process mode)".to_string(),
        format!("exec-main-status: {exec_status}"),
        format!("pid-file: {}", process_pid_path(config).display()),
        format!("log-file: {}", process_log_path(config).display()),
        format!("run-user: {}", config.agent_user),
        format!("listen: {}:{}", config.bind_host, config.bind_port),
        format!("tls-mode: {}", config.tls_mode),
        format!("tls-cert: {}", config.tls_cert_path),
        format!("tls-key: {}", config.tls_key_path),
        format!("url-hint: {url_hint}"),
        format!("health-hint: {health_hint}"),
    ];

    if !matches!(state, ProcessModeState::Running(_)) {
        lines.push("hint: run `computer-mcp start`".to_string());
    }
    if let Some(port) = config.http_bind_port {
        lines.push(format!("http-proxy-listen: {}:{port}", config.bind_host));
    }
    if !tls_artifacts_exist(config) {
        lines
            .push("note: `computer-mcp start` will create TLS artifacts automatically".to_string());
    }
    if matches!(state, ProcessModeState::Stale(_)) {
        lines.push(
            "hint: stale pid file detected; `computer-mcp restart` will cleanly recover"
                .to_string(),
        );
    }

    Ok(lines)
}

fn build_publisher_status_lines(
    config: &Config,
    state: Result<ProcessModeState>,
) -> Result<Vec<String>> {
    let state = state?;
    let active = match state {
        ProcessModeState::Running(_) => "active (running)",
        ProcessModeState::Stale(_) => "inactive (stale pid file)",
        ProcessModeState::Stopped => "inactive (dead)",
    };
    let exec_status = match state {
        ProcessModeState::Running(pid) => format!("running pid {pid}"),
        ProcessModeState::Stale(pid) => format!("stale pid file {pid}"),
        ProcessModeState::Stopped => "not running".to_string(),
    };

    let mut lines = vec![
        format!("service: {PUBLISHER_SERVICE_LABEL}"),
        "service-mode: process".to_string(),
        format!("active: {active}"),
        format!("exec-main-status: {exec_status}"),
        format!("pid-file: {}", publisher_process_pid_path(config).display()),
        format!("log-file: {}", publisher_process_log_path(config).display()),
        format!("run-user: {}", config.publisher_user),
        format!("socket: {}", config.publisher_socket_path),
        format!(
            "publisher-app-id: {}",
            config
                .publisher_app_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "<unset>".to_string())
        ),
        format!("publisher-key: {}", config.publisher_private_key_path),
        format!("allowed-repos: {}", config.publisher_targets.len()),
    ];

    if !matches!(state, ProcessModeState::Running(_)) {
        lines.push("hint: run `computer-mcp start`".to_string());
    }
    if config.publisher_app_id.is_none() {
        lines.push("hint: set `publisher_app_id` in config".to_string());
    }
    if !Path::new(&config.publisher_private_key_path).exists() {
        lines.push("hint: place the publisher private key at the configured path".to_string());
    }
    if config.publisher_targets.is_empty() {
        lines.push("hint: add at least one `publisher_targets` entry to config".to_string());
    }

    Ok(lines)
}

fn render_systemd_unit(daemon_path: &Path, config_path: &Path) -> String {
    let daemon_arg = quote_unit_arg(daemon_path);
    let config_arg = quote_unit_arg(config_path);

    format!(
        "[Unit]
Description=computer-mcp daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={daemon_arg} --config {config_arg}
Restart=always
RestartSec=2
NoNewPrivileges=true
Environment=RUST_LOG=computer_mcp=info,computer_mcpd=info

[Install]
WantedBy=multi-user.target
"
    )
}

fn quote_unit_arg(path: &Path) -> String {
    let escaped = path
        .display()
        .to_string()
        .replace('\\', r"\\")
        .replace('"', r#"\""#);
    format!("\"{escaped}\"")
}

fn write_if_changed(path: &Path, content: &str) -> Result<bool> {
    if let Ok(existing) = fs::read_to_string(path)
        && existing == content
    {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

#[derive(Debug, Clone, Copy)]
enum SystemctlAction {
    DaemonReload,
    Enable,
    Start,
    Stop,
    Restart,
    ShowStatus,
}

fn build_systemctl_args(action: SystemctlAction) -> Vec<String> {
    match action {
        SystemctlAction::DaemonReload => vec!["daemon-reload".to_string()],
        SystemctlAction::Enable => vec!["enable".to_string(), SERVICE_NAME.to_string()],
        SystemctlAction::Start => vec!["start".to_string(), SERVICE_NAME.to_string()],
        SystemctlAction::Stop => vec!["stop".to_string(), SERVICE_NAME.to_string()],
        SystemctlAction::Restart => vec!["restart".to_string(), SERVICE_NAME.to_string()],
        SystemctlAction::ShowStatus => vec![
            "show".to_string(),
            SERVICE_NAME.to_string(),
            "--property=ActiveState,SubState,UnitFileState,FragmentPath,ExecMainStatus".to_string(),
            "--no-pager".to_string(),
        ],
    }
}

fn build_journalctl_args() -> Vec<String> {
    vec![
        "-u".to_string(),
        SERVICE_NAME.to_string(),
        "-n".to_string(),
        DEFAULT_LOG_LINES.to_string(),
        "--no-pager".to_string(),
    ]
}

fn run_systemctl(args: &[String]) -> Result<String> {
    run_command_capture("systemctl", args)
}

fn run_journalctl(args: &[String]) -> Result<String> {
    run_command_capture("journalctl", args)
}

fn run_command_capture(program: &str, args: &[String]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {program}"))?;

    let stdout = redact_api_key_query_params(&String::from_utf8_lossy(&output.stdout));
    let stderr = redact_api_key_query_params(&String::from_utf8_lossy(&output.stderr));

    if output.status.success() {
        return Ok(stdout);
    }

    let status = output.status.code().map_or_else(
        || "terminated by signal".to_string(),
        |code| code.to_string(),
    );
    let details = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => "no output".to_string(),
        (false, true) => format!("stdout:\n{}", stdout.trim_end()),
        (true, false) => format!("stderr:\n{}", stderr.trim_end()),
        (false, false) => format!(
            "stdout:\n{}\n\nstderr:\n{}",
            stdout.trim_end(),
            stderr.trim_end()
        ),
    };

    bail!(
        "{program} {} failed (status: {status})\n{details}",
        args.join(" ")
    )
}

fn parse_systemctl_show(raw: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();

    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.to_string(), value.to_string());
        }
    }

    values
}

fn print_status_summary(raw: &str, config: &Config) {
    for line in build_status_summary_lines(raw, config, detect_public_ip()) {
        println!("{line}");
    }
}

fn build_status_summary_lines(
    raw: &str,
    config: &Config,
    public_ip: Option<IpAddr>,
) -> Vec<String> {
    let parsed = parse_systemctl_show(raw);
    let active = parsed
        .get("ActiveState")
        .map(String::as_str)
        .unwrap_or("unknown");
    let sub = parsed
        .get("SubState")
        .map(String::as_str)
        .unwrap_or("unknown");
    let unit_file_state = parsed
        .get("UnitFileState")
        .map(String::as_str)
        .unwrap_or("unknown");
    let fragment = parsed
        .get("FragmentPath")
        .map(String::as_str)
        .unwrap_or("unknown");
    let exec_status = parsed
        .get("ExecMainStatus")
        .map(String::as_str)
        .unwrap_or("unknown");

    let host_hint = status_host_hint(&config.bind_host, public_ip);
    let url_hint =
        redact_api_key_query_params(&format!("https://{host_hint}/mcp?key={}", config.api_key));
    let health_hint = format!("https://{host_hint}/health");

    let mut lines = vec![
        format!("service: {SERVICE_NAME}"),
        format!("active: {active} ({sub})"),
        format!("enabled: {unit_file_state}"),
        format!("unit-file: {fragment}"),
        format!("exec-main-status: {exec_status}"),
        format!("listen: {}:{}", config.bind_host, config.bind_port),
        format!("tls-mode: {}", config.tls_mode),
        format!("tls-cert: {}", config.tls_cert_path),
        format!("tls-key: {}", config.tls_key_path),
        format!("url-hint: {url_hint}"),
        format!("health-hint: {health_hint}"),
    ];

    if active != "active" {
        lines.push("hint: run `computer-mcp start`".to_string());
    }
    if let Some(port) = config.http_bind_port {
        lines.push(format!("http-proxy-listen: {}:{port}", config.bind_host));
    }
    if unit_file_state != "enabled" {
        lines.push("hint: run `computer-mcp install`".to_string());
    }
    if !tls_artifacts_exist(config) {
        lines
            .push("note: `computer-mcp start` will create TLS artifacts automatically".to_string());
    }
    lines
}

fn tls_artifacts_exist(config: &Config) -> bool {
    Path::new(&config.tls_cert_path).exists() && Path::new(&config.tls_key_path).exists()
}

fn detect_public_ip() -> Option<IpAddr> {
    if !command_exists("curl") {
        return None;
    }

    let output = Command::new("curl")
        .args(["-fsS", "--max-time", "4", "https://api.ipify.org"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    text.trim().parse::<IpAddr>().ok()
}

fn status_host_hint(bind_host: &str, public_ip: Option<IpAddr>) -> String {
    if bind_host.is_empty() || bind_host == "0.0.0.0" || bind_host == "::" || bind_host == "[::]" {
        return public_ip
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| STATUS_HOST_HINT_FALLBACK.to_string());
    }

    if let Ok(ip) = bind_host.parse::<IpAddr>()
        && ip.is_unspecified()
    {
        return public_ip
            .map(|candidate| candidate.to_string())
            .unwrap_or_else(|| STATUS_HOST_HINT_FALLBACK.to_string());
    }

    bind_host.to_string()
}

fn select_tls_san_ip(bind_host: &str, public_ip: Option<IpAddr>) -> IpAddr {
    if let Some(ip) = public_ip {
        return ip;
    }

    if let Ok(ip) = bind_host.parse::<IpAddr>()
        && !ip.is_unspecified()
    {
        return ip;
    }

    IpAddr::from([127, 0, 0, 1])
}

fn try_setup_letsencrypt_ip(config: &Config, ip: IpAddr) -> Result<()> {
    if !command_exists("certbot") {
        bail!("certbot is not installed");
    }

    let cert_name = certbot_cert_name(ip);
    run_command_capture("certbot", &build_certbot_args(ip, &cert_name))?;

    let (src_cert, src_key) = letsencrypt_live_paths(&cert_name);
    if !src_cert.exists() || !src_key.exists() {
        bail!(
            "expected certbot output files missing at {} and {}",
            src_cert.display(),
            src_key.display()
        );
    }

    copy_tls_files(
        &src_cert,
        &src_key,
        Path::new(&config.tls_cert_path),
        Path::new(&config.tls_key_path),
    )
}

fn certbot_cert_name(ip: IpAddr) -> String {
    format!("computer-mcp-{}", ip.to_string().replace(['.', ':'], "-"))
}

fn build_certbot_args(ip: IpAddr, cert_name: &str) -> Vec<String> {
    vec![
        "certonly".to_string(),
        "--standalone".to_string(),
        "--non-interactive".to_string(),
        "--agree-tos".to_string(),
        "--register-unsafely-without-email".to_string(),
        "--preferred-challenges".to_string(),
        "http".to_string(),
        "--keep-until-expiring".to_string(),
        "--cert-name".to_string(),
        cert_name.to_string(),
        "-d".to_string(),
        ip.to_string(),
    ]
}

fn letsencrypt_live_paths(cert_name: &str) -> (PathBuf, PathBuf) {
    let base = Path::new(LETSENCRYPT_LIVE_DIR).join(cert_name);
    (base.join("fullchain.pem"), base.join("privkey.pem"))
}

fn copy_tls_files(src_cert: &Path, src_key: &Path, dst_cert: &Path, dst_key: &Path) -> Result<()> {
    if let Some(parent) = dst_cert.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = dst_key.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::copy(src_cert, dst_cert).with_context(|| {
        format!(
            "failed to copy certificate from {} to {}",
            src_cert.display(),
            dst_cert.display()
        )
    })?;
    fs::copy(src_key, dst_key).with_context(|| {
        format!(
            "failed to copy private key from {} to {}",
            src_key.display(),
            dst_key.display()
        )
    })?;

    set_file_mode(dst_cert, 0o644)?;
    set_file_mode(dst_key, 0o600)?;
    Ok(())
}

fn generate_self_signed_certificate(config: &Config, ip: IpAddr) -> Result<()> {
    let mut params = CertificateParams::new(Vec::<String>::new())
        .context("failed to initialize self-signed cert parameters")?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, format!("computer-mcp {ip}"));
    params.distinguished_name = dn;
    params.subject_alt_names = vec![SanType::IpAddress(ip)];

    let key_pair = KeyPair::generate().context("failed to generate TLS key pair")?;
    let certificate = params
        .self_signed(&key_pair)
        .context("failed to generate self-signed certificate")?;
    let cert_pem = certificate.pem();
    let key_pem = key_pair.serialize_pem();

    let cert_path = Path::new(&config.tls_cert_path);
    let key_path = Path::new(&config.tls_key_path);
    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(cert_path, cert_pem)
        .with_context(|| format!("failed to write {}", cert_path.display()))?;
    fs::write(key_path, key_pem)
        .with_context(|| format!("failed to write {}", key_path.display()))?;
    set_file_mode(cert_path, 0o644)?;
    set_file_mode(key_path, 0o600)?;
    Ok(())
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> Result<()> {
    let mut perms = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?
        .permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)
        .with_context(|| format!("failed to set permissions for {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn restart_service_after_tls_setup(config: &Config, config_path: &Path) {
    match detect_service_manager() {
        ServiceManager::Systemd => {
            match run_systemctl(&build_systemctl_args(SystemctlAction::Restart)) {
                Ok(_) => println!("restarted {SERVICE_NAME} to apply TLS changes"),
                Err(err) => eprintln!(
                    "warning: TLS artifacts were updated but service restart failed. \
run `computer-mcp restart` manually.\n{err}"
                ),
            }
        }
        ServiceManager::Process => match process_mode_state(config) {
            Ok(ProcessModeState::Running(_)) => {
                if let Err(err) =
                    stop_process_mode(config).and_then(|_| start_process_mode(config, config_path))
                {
                    eprintln!(
                        "warning: TLS artifacts were updated but process-mode restart failed. \
run `computer-mcp --config \"{}\" restart` manually.\n{}",
                        config_path.display(),
                        err
                    );
                } else {
                    println!("restarted {SERVICE_NAME} in process mode to apply TLS changes");
                }
            }
            Ok(_) => {
                println!(
                    "TLS artifacts are ready. Start the stack with `computer-mcp --config \"{}\" start`.",
                    config_path.display(),
                );
            }
            Err(err) => eprintln!(
                "warning: TLS artifacts were updated but process-mode state check failed.\n{}",
                err
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_LOG_LINES, ProcessModeState, SERVICE_NAME, ServiceManager, SystemctlAction,
        build_certbot_args, build_journalctl_args, build_process_status_lines,
        build_publisher_status_lines, build_reader_status_lines, build_status_summary_lines,
        build_systemctl_args, build_upgrade_shell_args, certbot_cert_name,
        ensure_http_listener_ready_for_start, generate_self_signed_certificate,
        parse_systemctl_show, process_log_path, process_pid_path, read_tail_lines,
        render_systemd_unit, select_tls_san_ip, service_manager_from_pid1,
        shell_escape_single_quotes, state_root_for_config, status_host_hint, tls_artifacts_exist,
        write_if_changed,
    };
    use computer_mcp::config::Config;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    #[test]
    fn render_systemd_unit_contains_expected_execstart() {
        let unit = render_systemd_unit(
            Path::new("/usr/local/bin/computer-mcpd"),
            Path::new("/etc/computer-mcp/config.toml"),
        );
        assert!(unit.contains("[Service]"));
        assert!(unit.contains(
            "ExecStart=\"/usr/local/bin/computer-mcpd\" --config \"/etc/computer-mcp/config.toml\""
        ));
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("[Install]"));
    }

    #[test]
    fn build_systemctl_args_match_expected_shapes() {
        assert_eq!(
            build_systemctl_args(SystemctlAction::DaemonReload),
            vec!["daemon-reload"]
        );
        assert_eq!(
            build_systemctl_args(SystemctlAction::Enable),
            vec!["enable", SERVICE_NAME]
        );
        assert_eq!(
            build_systemctl_args(SystemctlAction::Start),
            vec!["start", SERVICE_NAME]
        );
        assert_eq!(
            build_systemctl_args(SystemctlAction::Stop),
            vec!["stop", SERVICE_NAME]
        );
        assert_eq!(
            build_systemctl_args(SystemctlAction::Restart),
            vec!["restart", SERVICE_NAME]
        );
        assert_eq!(
            build_systemctl_args(SystemctlAction::ShowStatus),
            vec![
                "show",
                SERVICE_NAME,
                "--property=ActiveState,SubState,UnitFileState,FragmentPath,ExecMainStatus",
                "--no-pager",
            ]
        );
    }

    #[test]
    fn build_journalctl_args_match_expected_shape() {
        assert_eq!(
            build_journalctl_args(),
            vec!["-u", SERVICE_NAME, "-n", DEFAULT_LOG_LINES, "--no-pager",]
        );
    }

    #[test]
    fn build_upgrade_shell_args_include_requested_version_and_http_port() {
        let mut config = Config::default();
        config.http_bind_port = Some(8080);

        let args = build_upgrade_shell_args("v0.1.5", &config);
        assert_eq!(args[0], "-lc");
        assert!(args[1].contains("export COMPUTER_MCP_VERSION='v0.1.5'"));
        assert!(args[1].contains("export COMPUTER_MCP_HTTP_BIND_PORT=8080"));
        assert!(
            args[1].contains(
                "curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | bash"
            )
        );
    }

    #[test]
    fn shell_escape_single_quotes_handles_embedded_quotes() {
        assert_eq!(shell_escape_single_quotes("v0.1.5's"), "'v0.1.5'\"'\"'s'");
    }

    #[test]
    fn parse_systemctl_show_extracts_values() {
        let raw = "ActiveState=active\nSubState=running\nUnitFileState=enabled\nFragmentPath=/etc/systemd/system/computer-mcpd.service\nExecMainStatus=0\n";
        let parsed = parse_systemctl_show(raw);

        assert_eq!(
            parsed.get("ActiveState").map(String::as_str),
            Some("active")
        );
        assert_eq!(parsed.get("SubState").map(String::as_str), Some("running"));
        assert_eq!(
            parsed.get("UnitFileState").map(String::as_str),
            Some("enabled")
        );
        assert_eq!(
            parsed.get("FragmentPath").map(String::as_str),
            Some("/etc/systemd/system/computer-mcpd.service")
        );
        assert_eq!(parsed.get("ExecMainStatus").map(String::as_str), Some("0"));
    }

    #[test]
    fn write_if_changed_is_idempotent() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("computer-mcpd.service");
        let content = "[Unit]\nDescription=test\n";

        let first = write_if_changed(&path, content).expect("first write");
        let second = write_if_changed(&path, content).expect("second write");

        assert!(first);
        assert!(!second);
        assert_eq!(fs::read_to_string(path).expect("read file"), content);
    }

    #[test]
    fn service_manager_from_pid1_detects_systemd() {
        assert_eq!(
            service_manager_from_pid1("systemd"),
            ServiceManager::Systemd
        );
        assert_eq!(
            service_manager_from_pid1("start.sh"),
            ServiceManager::Process
        );
    }

    #[test]
    fn state_root_for_config_uses_tls_parent_directory() {
        let mut config = Config::default();
        config.tls_cert_path = "/custom/state/tls/cert.pem".to_string();

        assert_eq!(
            state_root_for_config(&config),
            PathBuf::from("/custom/state")
        );
        assert_eq!(
            process_pid_path(&config),
            PathBuf::from("/custom/state/run/computer-mcpd.pid")
        );
        assert_eq!(
            process_log_path(&config),
            PathBuf::from("/custom/state/logs/computer-mcpd.log")
        );
    }

    #[test]
    fn read_tail_lines_returns_only_requested_suffix() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("computer-mcpd.log");
        fs::write(&path, "one\ntwo\nthree\nfour\n").expect("write log");

        let got = read_tail_lines(&path, 2).expect("read tail");
        assert_eq!(got, "three\nfour\n");
    }

    #[test]
    fn certbot_helpers_build_expected_values() {
        let ip: IpAddr = "203.0.113.42".parse().expect("ip parse");
        let cert_name = certbot_cert_name(ip);
        assert_eq!(cert_name, "computer-mcp-203-0-113-42");

        let args = build_certbot_args(ip, &cert_name);
        assert!(args.contains(&"certonly".to_string()));
        assert!(args.contains(&"--standalone".to_string()));
        assert!(args.contains(&"--non-interactive".to_string()));
        assert!(args.contains(&"--cert-name".to_string()));
        assert!(args.contains(&cert_name));
        assert!(args.contains(&ip.to_string()));
    }

    #[test]
    fn select_tls_san_ip_prefers_public_ip() {
        let public = Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7)));
        let selected = select_tls_san_ip("0.0.0.0", public);
        assert_eq!(selected, IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7)));
    }

    #[test]
    fn select_tls_san_ip_falls_back_to_bind_host() {
        let selected = select_tls_san_ip("192.0.2.10", None);
        assert_eq!(selected, IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)));
    }

    #[test]
    fn select_tls_san_ip_defaults_to_loopback() {
        let selected = select_tls_san_ip("0.0.0.0", None);
        assert_eq!(selected, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn status_host_hint_uses_public_ip_for_wildcard_bind() {
        let hint = status_host_hint("0.0.0.0", Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 11))));
        assert_eq!(hint, "203.0.113.11");
    }

    #[test]
    fn status_host_hint_uses_specific_bind_host() {
        let hint = status_host_hint("192.0.2.5", None);
        assert_eq!(hint, "192.0.2.5");
    }

    #[test]
    fn status_host_hint_returns_placeholder_without_public_ip() {
        let hint = status_host_hint("::", None);
        assert_eq!(hint, "<host>");
    }

    #[test]
    fn build_status_summary_lines_includes_network_and_tls_details() {
        let raw = "ActiveState=active\nSubState=running\nUnitFileState=enabled\nExecMainStatus=0\n";
        let mut config = Config::default();
        config.bind_host = "0.0.0.0".to_string();
        config.bind_port = 8443;
        config.http_bind_port = Some(8080);
        config.api_key = "abc123".to_string();
        config.tls_mode = "self_signed".to_string();
        config.tls_cert_path = "/var/lib/computer-mcp/tls/cert.pem".to_string();
        config.tls_key_path = "/var/lib/computer-mcp/tls/key.pem".to_string();

        let lines = build_status_summary_lines(
            raw,
            &config,
            Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 88))),
        );
        let joined = lines.join("\n");
        assert!(joined.contains("listen: 0.0.0.0:8443"));
        assert!(joined.contains("tls-mode: self_signed"));
        assert!(joined.contains("tls-cert: /var/lib/computer-mcp/tls/cert.pem"));
        assert!(joined.contains("tls-key: /var/lib/computer-mcp/tls/key.pem"));
        assert!(joined.contains("url-hint: https://198.51.100.88/mcp?key=<redacted>"));
        assert!(joined.contains("health-hint: https://198.51.100.88/health"));
        assert!(joined.contains("http-proxy-listen: 0.0.0.0:8080"));
    }

    #[test]
    fn build_process_status_lines_includes_process_mode_details() {
        let mut config = Config::default();
        config.bind_host = "0.0.0.0".to_string();
        config.bind_port = 9443;
        config.http_bind_port = Some(8080);
        config.api_key = "abc123".to_string();
        config.tls_mode = "self_signed".to_string();
        config.tls_cert_path = "/var/lib/computer-mcp/tls/cert.pem".to_string();
        config.tls_key_path = "/var/lib/computer-mcp/tls/key.pem".to_string();

        let lines = build_process_status_lines(
            &config,
            Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 88))),
            Ok(ProcessModeState::Running(4242)),
        )
        .expect("build process status");
        let joined = lines.join("\n");
        assert!(joined.contains("service-mode: process"));
        assert!(joined.contains("active: active (running)"));
        assert!(joined.contains("exec-main-status: running pid 4242"));
        assert!(joined.contains("url-hint: https://198.51.100.88/mcp?key=<redacted>"));
        assert!(joined.contains("health-hint: https://198.51.100.88/health"));
        assert!(joined.contains("http-proxy-listen: 0.0.0.0:8080"));
    }

    #[test]
    fn build_process_status_lines_suggests_recovery_for_stale_pid() {
        let config = Config::default();
        let lines = build_process_status_lines(&config, None, Ok(ProcessModeState::Stale(9999)))
            .expect("build process status");
        let joined = lines.join("\n");
        assert!(joined.contains("active: inactive (stale pid file)"));
        assert!(joined.contains(
            "hint: stale pid file detected; `computer-mcp restart` will cleanly recover"
        ));
    }

    #[test]
    fn build_publisher_status_lines_includes_socket_and_run_user() {
        let config = Config::default();
        let lines = build_publisher_status_lines(&config, Ok(ProcessModeState::Running(5150)))
            .expect("build publisher status");
        let joined = lines.join("\n");
        assert!(joined.contains("service: computer-mcp-prd"));
        assert!(joined.contains("run-user: computer-mcp-publisher"));
        assert!(
            joined.contains("socket: /var/lib/computer-mcp/publisher/run/computer-mcp-prd.sock")
        );
        assert!(joined.contains("allowed-repos: 0"));
        assert!(joined.contains("hint: set `publisher_app_id` in config"));
    }

    #[test]
    fn ensure_http_listener_ready_rejects_same_port_as_https() {
        let mut config = Config::default();
        config.bind_port = 443;
        config.http_bind_port = Some(443);

        let err = ensure_http_listener_ready_for_start(&config).expect_err("should fail");
        assert!(
            err.to_string()
                .contains("http_bind_port must differ from bind_port")
        );
    }

    #[test]
    fn build_status_summary_lines_notes_start_when_tls_files_missing() {
        let raw = "ActiveState=inactive\nSubState=dead\nUnitFileState=enabled\nExecMainStatus=1\n";
        let mut config = Config::default();
        let dir = tempdir().expect("tempdir");
        config.tls_cert_path = dir.path().join("missing-cert.pem").display().to_string();
        config.tls_key_path = dir.path().join("missing-key.pem").display().to_string();

        let lines = build_status_summary_lines(raw, &config, None);
        let joined = lines.join("\n");

        assert!(
            joined.contains("note: `computer-mcp start` will create TLS artifacts automatically")
        );
    }

    #[test]
    fn tls_artifacts_exist_checks_both_files() {
        let dir = tempdir().expect("tempdir");
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");
        fs::write(&cert, "cert").expect("write cert");

        let mut config = Config::default();
        config.tls_cert_path = cert.display().to_string();
        config.tls_key_path = key.display().to_string();
        assert!(!tls_artifacts_exist(&config));

        fs::write(&key, "key").expect("write key");
        assert!(tls_artifacts_exist(&config));
    }

    #[test]
    fn generate_self_signed_certificate_writes_pem_files() {
        let dir = tempdir().expect("tempdir");
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");

        let mut config = Config::default();
        config.tls_cert_path = cert_path.display().to_string();
        config.tls_key_path = key_path.display().to_string();

        generate_self_signed_certificate(&config, IpAddr::V6(Ipv6Addr::LOCALHOST))
            .expect("generate self signed cert");

        let cert = fs::read_to_string(&cert_path).expect("read cert");
        let key = fs::read_to_string(&key_path).expect("read key");
        assert!(cert.contains("BEGIN CERTIFICATE"));
        assert!(key.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn build_reader_status_lines_include_reader_hints() {
        let config = Config::default();
        let joined = build_reader_status_lines(&config).join("\n");
        assert!(joined.contains("service: computer-mcp-reader"));
        assert!(joined.contains("active: not-ready"));
        assert!(joined.contains("hint: set `reader_app_id` in config"));
        assert!(joined.contains("hint: set `reader_installation_id` in config"));
    }
}
