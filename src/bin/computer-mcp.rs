use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use computer_mcp::config::{Config, DEFAULT_CONFIG_PATH};
use rand::distr::{Alphanumeric, SampleString};

const SERVICE_NAME: &str = "computer-mcpd.service";
const SYSTEMD_UNIT_PATH: &str = "/etc/systemd/system/computer-mcpd.service";
const STATE_DIR: &str = "/var/lib/computer-mcp";
const TLS_DIR: &str = "/var/lib/computer-mcp/tls";
const DEFAULT_LOG_LINES: &str = "200";

#[derive(Debug, Parser)]
#[command(name = "computer-mcp")]
#[command(about = "Computer MCP CLI")]
struct Cli {
    #[arg(long, default_value = DEFAULT_CONFIG_PATH)]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install,
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
}

#[derive(Debug, Subcommand)]
enum TlsCommand {
    Setup,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = PathBuf::from(&cli.config);

    match cli.command {
        Commands::Install => {
            install(&config_path)?;
        }
        Commands::Start => {
            ensure_linux()?;
            run_systemctl(&build_systemctl_args(SystemctlAction::Start))?;
            println!("started {SERVICE_NAME}");
        }
        Commands::Stop => {
            ensure_linux()?;
            run_systemctl(&build_systemctl_args(SystemctlAction::Stop))?;
            println!("stopped {SERVICE_NAME}");
        }
        Commands::Restart => {
            ensure_linux()?;
            run_systemctl(&build_systemctl_args(SystemctlAction::Restart))?;
            println!("restarted {SERVICE_NAME}");
        }
        Commands::Status => {
            ensure_linux()?;
            let raw = run_systemctl(&build_systemctl_args(SystemctlAction::ShowStatus))?;
            print_status_summary(&raw);
        }
        Commands::Logs => {
            ensure_linux()?;
            let logs = run_journalctl(&build_journalctl_args())?;
            if logs.is_empty() {
                println!("no recent logs found for {SERVICE_NAME}");
            } else {
                print!("{logs}");
            }
        }
        Commands::SetKey { value } => {
            let mut config = Config::load(Some(Path::new(&config_path)))?;
            config.api_key = value;
            config.save(&config_path)?;
            println!("updated API key in {}", config_path.display());
        }
        Commands::RotateKey => {
            let mut config = Config::load(Some(Path::new(&config_path)))?;
            let mut rng = rand::rng();
            config.api_key = Alphanumeric.sample_string(&mut rng, 48);
            config.save(&config_path)?;
            println!("rotated API key in {}", config_path.display());
        }
        Commands::ShowUrl { host } => {
            let config = Config::load(Some(Path::new(&config_path)))?;
            println!("https://{host}/mcp?key={}", config.api_key);
        }
        Commands::Tls { command } => match command {
            TlsCommand::Setup => {
                println!("tls setup scaffold ready: LE IP + self-signed fallback is pending");
            }
        },
    }

    Ok(())
}

fn install(config_path: &Path) -> Result<()> {
    ensure_linux()?;
    create_required_dirs(config_path)?;
    ensure_config_exists(config_path)?;

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
    Ok(())
}

fn ensure_linux() -> Result<()> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        bail!("computer-mcp CLI service management is Linux-only");
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

fn ensure_config_exists(config_path: &Path) -> Result<()> {
    if config_path.exists() {
        return Ok(());
    }

    Config::default().save(config_path)?;
    println!("created default config at {}", config_path.display());
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

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

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

fn print_status_summary(raw: &str) {
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

    println!("service: {SERVICE_NAME}");
    println!("active: {active} ({sub})");
    println!("enabled: {unit_file_state}");
    println!("unit-file: {fragment}");
    println!("exec-main-status: {exec_status}");

    if active != "active" {
        println!("hint: run `computer-mcp start`");
    }
    if unit_file_state != "enabled" {
        println!("hint: run `computer-mcp install`");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_LOG_LINES, SERVICE_NAME, SystemctlAction, build_journalctl_args,
        build_systemctl_args, parse_systemctl_show, render_systemd_unit, write_if_changed,
    };
    use std::fs;
    use std::path::Path;
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
}
