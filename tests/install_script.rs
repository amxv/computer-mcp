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
        "COMPUTER_MCP_AGENT_HOME",
        "COMPUTER_MCP_AGENT_SHELL",
        "COMPUTER_MCP_DEFAULT_WORKDIR",
        "COMPUTER_MCP_PUBLISHER_USER",
        "COMPUTER_MCP_PUBLISHER_HOME",
        "COMPUTER_MCP_SERVICE_GROUP",
        "COMPUTER_MCP_GIT_USER_NAME",
        "COMPUTER_MCP_GIT_USER_EMAIL",
        "COMPUTER_MCP_READER_KEY_DIR",
        "COMPUTER_MCP_HTTP_BIND_PORT",
        "COMPUTER_MCP_PUBLIC_HOST",
        "ensure_service_accounts()",
        "detect_platform()",
        "resolved_http_bind_port()",
        "resolved_public_host()",
        "install_runtime_prerequisites()",
        "install_build_prerequisites()",
        "resolve_release_asset_url()",
        "server_archive_name=\"computer-mcp-${TARGET_TRIPLE}.tar.gz\"",
        "[^\\\"]*/${server_archive_name}\\\"",
        "install_binaries_from_release()",
        "install_binaries_from_source()",
        "run_cli_install()",
        "configure_agent_git_identity()",
        "configure_agent_git_reader_helper()",
        "git config --global user.name",
        "git config --global user.email",
        "${COMPUTER_MCP_STATE_DIR}/publisher/run",
        "${COMPUTER_MCP_STATE_DIR}/publisher/logs",
        "credential.https://github.com.helper",
        "git-credential-helper",
        "print_next_steps()",
        "apt-get install -y --no-install-recommends",
        "build-essential pkg-config libssl-dev git",
        "computer-mcp-prd",
        "agent_home = \"${COMPUTER_MCP_AGENT_HOME}\"",
        "default_workdir = \"${COMPUTER_MCP_DEFAULT_WORKDIR}\"",
        "The commands below assume the default config path",
        "Most installs can keep the built-in defaults.",
        "reader_app_id",
        "reader_installation_id",
        "rotate the installer-generated API key",
        "curl -k \"https://${public_host}/health\"",
        "MCP URL shape: https://${public_host}/mcp?key=<redacted>",
        "expose HTTP port ${http_port}",
    ];

    for snippet in required_snippets {
        assert!(
            script.contains(snippet),
            "install script missing snippet: {snippet}"
        );
    }
}

#[test]
fn install_script_does_not_use_generic_target_triple_tarball_match() {
    let script = std::fs::read_to_string(install_script_path()).expect("read install script");

    assert!(
        !script.contains("${TARGET_TRIPLE}[^\"]*\\.tar\\.gz"),
        "install script should not select release assets via generic target triple tarball match"
    );
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
