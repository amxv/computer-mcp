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
        "detect_platform()",
        "install_prerequisites()",
        "install_binaries_from_release()",
        "install_binaries_from_source()",
        "run_cli_install()",
        "print_next_steps()",
        "apt-get install -y --no-install-recommends",
        "tls setup",
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
