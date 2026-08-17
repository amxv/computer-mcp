fn derive_remote_target_repo(
    sprite: &str,
    org: Option<&str>,
    remote_config: &Path,
) -> Result<Option<String>> {
    let exec_args = vec![
        "sudo".to_string(),
        "awk".to_string(),
        "-F\"".to_string(),
        r#"/^\[\[publisher_targets\]\]/ { in_targets=1; next } in_targets && /^repo = "/ { print $2; exit }"#.to_string(),
        remote_config.display().to_string(),
    ];
    let raw = run_sprite_exec(sprite, org, &exec_args, &[])?;
    let repo = raw.trim();
    if repo.is_empty() {
        Ok(None)
    } else {
        Ok(Some(repo.to_string()))
    }
}

fn verify_agent_git_identity(sprite: &str, org: Option<&str>) -> Result<()> {
    let script = r#"set -euo pipefail
smoke_dir=/workspace/.git-identity-zodex-smoke
rm -rf "$smoke_dir"
git init -q "$smoke_dir"
cd "$smoke_dir"
printf "sprite git identity smoke\n" > smoke.txt
git add smoke.txt
git commit -q -m "Smoke: verify default agent git identity"
git log -1 --format="%an <%ae>"
cd /workspace
rm -rf "$smoke_dir"
"#;
    let exec_args = vec![
        "sudo".to_string(),
        "-u".to_string(),
        "zodex-agent".to_string(),
        "env".to_string(),
        "HOME=/home/zodex-agent".to_string(),
        "bash".to_string(),
        "-lc".to_string(),
        script.to_string(),
    ];
    run_sprite_exec(sprite, org, &exec_args, &[])?;
    Ok(())
}

fn verify_reader_git_access(sprite: &str, org: Option<&str>, repo: &str) -> Result<()> {
    let exec_args = vec![
        "sudo".to_string(),
        "-u".to_string(),
        "zodex-agent".to_string(),
        "env".to_string(),
        "HOME=/home/zodex-agent".to_string(),
        "git".to_string(),
        "ls-remote".to_string(),
        format!("https://github.com/{repo}.git"),
        "HEAD".to_string(),
    ];
    run_sprite_exec(sprite, org, &exec_args, &[])?;
    Ok(())
}

fn verify_publisher_socket_permissions(sprite: &str, org: Option<&str>) -> Result<()> {
    let script = r#"set -euo pipefail
dir_path=/var/lib/zodex/publisher/run
sock_path=/var/lib/zodex/publisher/run/zodex-prd.sock
[[ "$(stat -c %a "$dir_path")" == "750" ]]
[[ "$(stat -c %U "$dir_path")" == "zodex-publisher" ]]
[[ "$(stat -c %G "$dir_path")" == "zodex" ]]
[[ "$(stat -c %a "$sock_path")" == "660" ]]
[[ "$(stat -c %U "$sock_path")" == "zodex-publisher" ]]
[[ "$(stat -c %G "$sock_path")" == "zodex" ]]
"#;
    let exec_args = vec![
        "sudo".to_string(),
        "bash".to_string(),
        "-lc".to_string(),
        script.to_string(),
    ];
    run_sprite_exec(sprite, org, &exec_args, &[])?;
    Ok(())
}

fn verify_publisher_key_isolation(sprite: &str, org: Option<&str>) -> Result<()> {
    let script = r#"cat /etc/zodex/publisher/private-key.pem >/dev/null 2>&1"#;
    let exec_args = vec![
        "sudo".to_string(),
        "-u".to_string(),
        "zodex-agent".to_string(),
        "env".to_string(),
        "HOME=/home/zodex-agent".to_string(),
        "bash".to_string(),
        "-lc".to_string(),
        script.to_string(),
    ];
    match run_sprite_exec(sprite, org, &exec_args, &[]) {
        Ok(_) => bail!(
            "zodex-agent unexpectedly gained read access to /etc/zodex/publisher/private-key.pem"
        ),
        Err(_) => Ok(()),
    }
}
