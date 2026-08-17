fn upgrade_operator(version: &str) -> Result<()> {
    run_command_capture("bash", &build_operator_upgrade_shell_args(version))?;
    Ok(())
}

fn build_operator_upgrade_shell_args(version: &str) -> Vec<String> {
    let script = format!(
        "set -euo pipefail\nexport ZODEX_INSTALL_MODE=operator\nexport ZODEX_VERSION={}\ncurl -fsSL 'https://zodex.ashray.xyz/install.sh' | bash",
        shell_escape_single_quotes(version)
    );
    vec!["-lc".to_string(), script]
}
