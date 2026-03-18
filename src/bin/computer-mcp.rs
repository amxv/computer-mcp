use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use computer_mcp::config::{Config, DEFAULT_CONFIG_PATH};
use computer_mcp::redaction::redact_api_key_query_params;
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
            let config = Config::load(Some(Path::new(&config_path)))?;
            ensure_tls_ready_for_start(&config, &config_path)?;
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
            let config = Config::load(Some(Path::new(&config_path)))?;
            let raw = run_systemctl(&build_systemctl_args(SystemctlAction::ShowStatus))?;
            print_status_summary(&raw, &config);
        }
        Commands::Logs => {
            ensure_linux()?;
            let logs = run_journalctl(&build_journalctl_args())?;
            if logs.is_empty() {
                println!("no recent logs found for {SERVICE_NAME}");
            } else {
                print!("{}", redact_api_key_query_params(&logs));
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
            let raw_url = format!("https://{host}/mcp?key={}", config.api_key);
            println!(
                "{} (key redacted in CLI output)",
                redact_api_key_query_params(&raw_url)
            );
        }
        Commands::Tls { command } => match command {
            TlsCommand::Setup => tls_setup(&config_path)?,
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

fn tls_setup(config_path: &Path) -> Result<()> {
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
    println!("updated TLS settings in {}", config_path.display());
    restart_service_after_tls_setup();
    Ok(())
}

fn ensure_linux() -> Result<()> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        bail!("computer-mcp CLI service management is Linux-only");
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
    if unit_file_state != "enabled" {
        lines.push("hint: run `computer-mcp install`".to_string());
    }
    if !tls_artifacts_exist(config) {
        lines.push("hint: run `computer-mcp tls setup`".to_string());
    }
    lines
}

fn tls_artifacts_exist(config: &Config) -> bool {
    Path::new(&config.tls_cert_path).exists() && Path::new(&config.tls_key_path).exists()
}

fn ensure_tls_ready_for_start(config: &Config, config_path: &Path) -> Result<()> {
    if tls_artifacts_exist(config) {
        return Ok(());
    }

    let cert_missing = !Path::new(&config.tls_cert_path).exists();
    let key_missing = !Path::new(&config.tls_key_path).exists();

    let mut missing = Vec::new();
    if cert_missing {
        missing.push(format!("cert: {}", config.tls_cert_path));
    }
    if key_missing {
        missing.push(format!("key: {}", config.tls_key_path));
    }

    bail!(
        "TLS artifacts are required before start and are missing ({})\nrun `computer-mcp --config \"{}\" tls setup` and retry",
        missing.join(", "),
        config_path.display()
    )
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

fn restart_service_after_tls_setup() {
    match run_systemctl(&build_systemctl_args(SystemctlAction::Restart)) {
        Ok(_) => println!("restarted {SERVICE_NAME} to apply TLS changes"),
        Err(err) => eprintln!(
            "warning: TLS artifacts were updated but service restart failed. \
run `computer-mcp restart` manually.\n{err}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_LOG_LINES, SERVICE_NAME, SystemctlAction, build_certbot_args,
        build_journalctl_args, build_status_summary_lines, build_systemctl_args, certbot_cert_name,
        ensure_tls_ready_for_start, generate_self_signed_certificate, parse_systemctl_show,
        render_systemd_unit, select_tls_san_ip, status_host_hint, tls_artifacts_exist,
        write_if_changed,
    };
    use computer_mcp::config::Config;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
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
    }

    #[test]
    fn build_status_summary_lines_suggests_tls_setup_when_files_missing() {
        let raw = "ActiveState=inactive\nSubState=dead\nUnitFileState=enabled\nExecMainStatus=1\n";
        let mut config = Config::default();
        let dir = tempdir().expect("tempdir");
        config.tls_cert_path = dir.path().join("missing-cert.pem").display().to_string();
        config.tls_key_path = dir.path().join("missing-key.pem").display().to_string();

        let lines = build_status_summary_lines(raw, &config, None);
        let joined = lines.join("\n");

        assert!(joined.contains("hint: run `computer-mcp tls setup`"));
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
    fn ensure_tls_ready_for_start_returns_actionable_error() {
        let dir = tempdir().expect("tempdir");
        let mut config = Config::default();
        config.tls_cert_path = dir.path().join("missing-cert.pem").display().to_string();
        config.tls_key_path = dir.path().join("missing-key.pem").display().to_string();

        let err = ensure_tls_ready_for_start(&config, Path::new("/etc/computer-mcp/config.toml"))
            .expect_err("expected missing tls error");
        let msg = err.to_string();
        assert!(msg.contains("TLS artifacts are required before start"));
        assert!(msg.contains("computer-mcp --config \"/etc/computer-mcp/config.toml\" tls setup"));
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
}
