use super::local_network::LOCAL_NETWORK_NAMESPACE;
use super::*;

pub(super) const LOCAL_TUNNEL_VERSION: &str = "v0.0.11";
pub(super) const LOCAL_TUNNEL_ARCHIVE_URL: &str =
    "https://persistent.oaistatic.com/tunnel-client/v0.0.11/tunnel-client-v0.0.11-linux-arm64.zip";
pub(super) const LOCAL_TUNNEL_ARCHIVE_SHA256: &str =
    "d8bba47b2a723799a372b0b87d7e4d69304093d3a28837237315fe5406d97e77";
pub(super) const LOCAL_TUNNEL_BINARY_PATH: &str = "/usr/local/bin/tunnel-client";
pub(super) const LOCAL_TUNNEL_SERVICE_NAME: &str = "zodex-tunnel.service";
pub(super) const LOCAL_TUNNEL_UNIT_PATH: &str = "/etc/systemd/system/zodex-tunnel.service";
pub(super) const LOCAL_TUNNEL_RUNTIME_KEY_PATH: &str = "/etc/zodex/tunnel/runtime-key";
pub(super) const LOCAL_TUNNEL_RUNTIME_KEY_TMP_PATH: &str = "/tmp/zodex-local-tunnel-runtime-key";
pub(super) const LOCAL_TUNNEL_CONFIG_PATH: &str = "/etc/zodex/tunnel/config.yaml";
pub(super) const LOCAL_TUNNEL_MCP_BEARER_PATH: &str = "/etc/zodex/tunnel/mcp-bearer";
pub(super) const LOCAL_TUNNEL_VERSION_PATH: &str = "/etc/zodex/tunnel/version";
pub(super) const LOCAL_TUNNEL_HEALTH_URL: &str = "http://127.0.0.1:18080";

const LOCAL_TUNNEL_UNIT: &str = include_str!("local_zodex_tunnel.service");

pub(super) fn validate_local_tunnel_id(tunnel_id: &str) -> Result<()> {
    let Some(suffix) = tunnel_id.strip_prefix("tunnel_") else {
        bail!("tunnel ID must start with `tunnel_`");
    };
    if suffix.len() != 32
        || !suffix
            .chars()
            .all(|ch| ch.is_ascii_digit() || ('a'..='f').contains(&ch))
    {
        bail!("tunnel ID must be `tunnel_` followed by 32 lowercase hexadecimal characters");
    }
    Ok(())
}

pub(super) fn local_tunnel_ready_command() -> Vec<String> {
    vec![
        "/usr/sbin/ip".into(),
        "netns".into(),
        "exec".into(),
        LOCAL_NETWORK_NAMESPACE.into(),
        "/usr/bin/curl".into(),
        "-fsS".into(),
        format!("{LOCAL_TUNNEL_HEALTH_URL}/readyz"),
    ]
}

pub(super) fn build_local_tunnel_install_fragment(tunnel_id: &str) -> Result<String> {
    validate_local_tunnel_id(tunnel_id)?;
    Ok(format!(
        r#"
TUNNEL_ID={tunnel_id}
TUNNEL_VERSION={version}
TUNNEL_ARCHIVE="$(mktemp)"
TUNNEL_UNPACK="$(mktemp -d)"
curl -fsSL {archive_url} -o "$TUNNEL_ARCHIVE"
printf '%s  %s\n' '{archive_sha}' "$TUNNEL_ARCHIVE" | sha256sum -c -
unzip -q "$TUNNEL_ARCHIVE" -d "$TUNNEL_UNPACK"
install -m 0755 -o root -g root "$TUNNEL_UNPACK/tunnel-client" {binary_path}
rm -rf "$TUNNEL_ARCHIVE" "$TUNNEL_UNPACK"
printf '%s\n' "$TUNNEL_VERSION" > {version_path}
chown root:zodex-tunnel {version_path}
chmod 0440 {version_path}

install -m 0600 -o zodex-tunnel -g zodex-tunnel {runtime_key_tmp} {runtime_key_path}
python3 - "$CFG" "$TUNNEL_ID" {config_path} <<'PY'
import pathlib
import sys
import urllib.parse

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

config_path = pathlib.Path(sys.argv[1])
tunnel_id = sys.argv[2]
output_path = pathlib.Path(sys.argv[3])
with config_path.open("rb") as handle:
    config = tomllib.load(handle)
api_key = config.get("api_key")
if not isinstance(api_key, str) or not api_key:
    raise SystemExit("zodex config is missing api_key")
server_url = "http://127.0.0.1:8080/mcp?key=" + urllib.parse.quote(api_key, safe="")
bearer_path = pathlib.Path("{mcp_bearer_path}")
bearer_path.write_text("Bearer " + api_key, encoding="utf-8")
output_path.write_text(
    "config_version: 1\n"
    "control_plane:\n"
    f"  tunnel_id: {{tunnel_id}}\n"
    "  api_key: file:/etc/zodex/tunnel/runtime-key\n"
    "log:\n"
    "  level: info\n"
    "  format: json\n"
    "health:\n"
    "  listen_addr: 127.0.0.1:18080\n"
    "  url_file: /run/zodex-tunnel/health-url\n"
    "process:\n"
    "  pid_file: /run/zodex-tunnel/tunnel-client.pid\n"
    "mcp:\n"
    "  discovery_extra_headers:\n"
    "    Authorization: \"file:{mcp_bearer_path}\"\n"
    "  server_urls:\n"
    "    - channel: main\n"
    f"      url: {{server_url}}\n",
    encoding="utf-8",
)
PY
chown root:zodex-tunnel {config_path}
chmod 0640 {config_path}
chown root:zodex-tunnel {mcp_bearer_path}
chmod 0640 {mcp_bearer_path}
cat > {unit_path} <<'EOF'
{unit}EOF
systemctl disable {service_name} 2>/dev/null || true
"#,
        tunnel_id = shell_escape_single_quotes(tunnel_id),
        version = shell_escape_single_quotes(LOCAL_TUNNEL_VERSION),
        archive_url = LOCAL_TUNNEL_ARCHIVE_URL,
        archive_sha = LOCAL_TUNNEL_ARCHIVE_SHA256,
        binary_path = LOCAL_TUNNEL_BINARY_PATH,
        version_path = LOCAL_TUNNEL_VERSION_PATH,
        runtime_key_tmp = LOCAL_TUNNEL_RUNTIME_KEY_TMP_PATH,
        runtime_key_path = LOCAL_TUNNEL_RUNTIME_KEY_PATH,
        config_path = LOCAL_TUNNEL_CONFIG_PATH,
        mcp_bearer_path = LOCAL_TUNNEL_MCP_BEARER_PATH,
        unit_path = LOCAL_TUNNEL_UNIT_PATH,
        unit = LOCAL_TUNNEL_UNIT,
        service_name = LOCAL_TUNNEL_SERVICE_NAME,
    ))
}
