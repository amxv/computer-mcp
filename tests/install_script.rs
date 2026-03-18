use std::path::PathBuf;
use std::process::Command;

fn install_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("install.sh")
}

#[test]
fn install_script_has_expected_structure() {
    let script = std::fs::read_to_string(install_script_path()).expect("read install script");

    let required_snippets = [
        "set -euo pipefail",
        "COMPUTER_MCP_VERSION",
        "COMPUTER_MCP_ASSET_URL",
        "COMPUTER_MCP_BINARY_SOURCE_DIR",
        "COMPUTER_MCP_INSTALL_DIR",
        "COMPUTER_MCP_CONFIG_PATH",
        "COMPUTER_MCP_AGENT_USER",
        "COMPUTER_MCP_PUBLISHER_USER",
        "COMPUTER_MCP_SERVICE_GROUP",
        "COMPUTER_MCP_READER_KEY_DIR",
        "ensure_service_accounts()",
        "detect_platform()",
        "install_runtime_prerequisites()",
        "install_build_prerequisites()",
        "install_binaries_from_release()",
        "install_binaries_from_source()",
        "run_cli_install()",
        "print_next_steps()",
        "apt-get install -y --no-install-recommends",
        "build-essential pkg-config libssl-dev git",
        "computer-mcp-prd",
        "The commands below assume the default config path",
        "Most installs can keep the built-in defaults.",
        "reader_app_id",
        "reader_installation_id",
        "rotate the installer-generated API key",
        "curl -k \"https://${ip}/health\"",
        "MCP URL shape: https://${ip}/mcp?key=<redacted>",
    ];

    for snippet in required_snippets {
        assert!(
            script.contains(snippet),
            "install script missing snippet: {snippet}"
        );
    }
}

#[test]
fn install_script_is_valid_bash_syntax() {
    let output = Command::new("bash")
        .arg("-n")
        .arg(install_script_path())
        .output()
        .expect("run bash -n");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("bash -n failed: {stderr}");
    }
}
