use std::process::Command;

#[test]
fn zodex_github_help_exposes_mode_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["github", "--help"])
        .output()
        .expect("run zodex github --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("request-push"));
    assert!(stdout.contains("grant-push"));
    assert!(stdout.contains("revoke-push"));
    assert!(stdout.contains("list-grants"));
    assert!(stdout.contains("mode"));
}

#[test]
fn zodex_github_mode_help_exposes_yolo_default_and_status() {
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["github", "mode", "--help"])
        .output()
        .expect("run zodex github mode --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("yolo"));
    assert!(stdout.contains("default"));
    assert!(stdout.contains("status"));
}

#[test]
fn zodex_github_mode_yolo_help_exposes_expected_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["github", "mode", "yolo", "--help"])
        .output()
        .expect("run zodex github mode yolo --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("--local"));
    assert!(stdout.contains("--sprite"));
    assert!(stdout.contains("--org"));
    assert!(stdout.contains("--repo"));
    assert!(stdout.contains("--ttl"));
    assert!(stdout.contains("--no-ttl"));
    assert!(stdout.contains("[default: 2h]"));
}

#[test]
fn zodex_github_mode_default_and_status_expose_local_selector() {
    for command in ["default", "status"] {
        let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
            .args(["github", "mode", command, "--help"])
            .output()
            .expect("run github mode subcommand help");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("--local"), "missing --local on {command}");
        assert!(stdout.contains("--sprite"), "missing --sprite on {command}");
        assert!(stdout.contains("--org"), "missing --org on {command}");
    }
}

#[test]
fn zodex_github_mode_local_and_sprite_selectors_are_mutually_exclusive() {
    for args in [
        vec!["github", "mode", "yolo", "--local", "--sprite", "dev"],
        vec!["github", "mode", "default", "--local", "--sprite", "dev"],
        vec!["github", "mode", "status", "--local", "--sprite", "dev"],
        vec!["github", "mode", "status", "--local", "--org", "team"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
            .args(&args)
            .output()
            .expect("run conflicting github mode selectors");
        assert!(
            !output.status.success(),
            "conflicting selectors unexpectedly parsed: {args:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cannot be used with"),
            "unexpected clap error: {stderr}"
        );
    }
}

#[test]
fn zodex_local_help_exposes_reset_and_keeps_worker_hidden() {
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "--help"])
        .output()
        .expect("run zodex local --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("setup"));
    assert!(stdout.contains("exec"));
    assert!(stdout.contains("start"));
    assert!(stdout.contains("stop"));
    assert!(stdout.contains("reset"));
    assert!(stdout.contains("Permanently erase Local machine storage"));
    assert!(stdout.contains("status"));
    assert!(!stdout.contains("lease-worker"));
}

#[test]
fn zodex_local_reset_without_saved_setup_fails_before_state_mutation() {
    let home = tempfile::tempdir().expect("temp HOME");
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "reset"])
        .env("HOME", home.path())
        .output()
        .expect("run unconfigured local reset");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not configured"));
    assert!(!home.path().join(".config/zodex/local-target.json").exists());
    assert!(
        !home
            .path()
            .join(".config/zodex/local-access-lease.json")
            .exists()
    );
    assert!(
        !home
            .path()
            .join(".config/zodex/local-last-ready-setup.json")
            .exists()
    );
}

#[test]
fn zodex_local_start_requires_finite_ttl_and_has_no_no_ttl_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "start", "--help"])
        .output()
        .expect("run zodex local start --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--ttl <TTL>"));
    assert!(!stdout.contains("--no-ttl"));

    let missing = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "start"])
        .output()
        .expect("run zodex local start without ttl");
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("--ttl <TTL>"));
}

#[test]
fn zodex_local_status_is_read_only_and_truthful_before_configuration() {
    let home = tempfile::tempdir().expect("temp HOME");
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "status"])
        .env("HOME", home.path())
        .output()
        .expect("run zodex local status");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Local target: zodex-local"));
    assert!(stdout.contains("Configuration: not configured"));
    assert!(stdout.contains("Provider:"));
    assert!(stdout.contains("MCP access: inactive"));
    assert!(!home.path().join(".config/zodex/local-target.json").exists());
    assert!(
        !home
            .path()
            .join(".config/zodex/local-access-lease.json")
            .exists()
    );
}

#[test]
fn zodex_local_setup_fails_before_mutation_on_unsupported_platform() {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return;
    }
    let home = tempfile::tempdir().expect("temp HOME");
    let reader = home.path().join("reader.pem");
    let publisher = home.path().join("publisher.pem");
    let tunnel_key = home.path().join("tunnel-runtime-key");
    std::fs::write(&reader, "fixture").expect("reader fixture");
    std::fs::write(&publisher, "fixture").expect("publisher fixture");
    std::fs::write(&tunnel_key, "fixture-runtime-key").expect("tunnel key fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args([
            "local",
            "setup",
            "--repo",
            "amxv/zodex",
            "--reader-app-id",
            "1",
            "--reader-pem",
        ])
        .arg(&reader)
        .args(["--publisher-app-id", "2", "--publisher-pem"])
        .arg(&publisher)
        .args([
            "--tunnel-id",
            "tunnel_0123456789abcdef0123456789abcdef",
            "--tunnel-runtime-key",
        ])
        .arg(&tunnel_key)
        .env("HOME", home.path())
        .output()
        .expect("run unsupported local setup");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Local is unsupported"));
    assert!(!home.path().join(".config/zodex/local-target.json").exists());
}
