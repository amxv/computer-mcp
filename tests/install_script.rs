use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

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
        "ZODEX_VERSION",
        "ZODEX_INSTALL_MODE",
        "detect_operator_platform()",
        "run_operator_install()",
        "run_runtime_install()",
        "sha256_verify()",
        "resolve_release_checksum_url()",
        "operator_local_runtime_dir()",
        "ensure_local_stopped_before_operator_replace()",
        "install_operator_binary_atomically()",
        "/bin/rm -rf \"${TMP_DIR}\"",
        "zodex operator CLI installed.",
        "ZODEX_ASSET_URL",
        "ZODEX_BINARY_SOURCE_DIR",
        "ZODEX_INSTALL_DIR",
        "ZODEX_CONFIG_PATH",
        "ZODEX_AGENT_USER",
        "ZODEX_AGENT_HOME",
        "ZODEX_AGENT_SHELL",
        "ZODEX_DEFAULT_WORKDIR",
        "ZODEX_PUBLISHER_USER",
        "ZODEX_PUBLISHER_HOME",
        "ZODEX_SERVICE_GROUP",
        "ZODEX_GIT_USER_NAME",
        "ZODEX_GIT_USER_EMAIL",
        "ZODEX_READER_KEY_DIR",
        "ZODEX_SERVICE_PORT",
        "ensure_service_accounts()",
        "detect_platform()",
        "install_runtime_prerequisites()",
        "install_build_prerequisites()",
        "resolve_release_asset_url()",
        "server_archive_name=\"zodex-${TARGET_TRIPLE}.tar.gz\"",
        "install_binaries_from_release()",
        "install_binaries_from_source()",
        "configure_agent_git_identity()",
        "configure_agent_git_reader_helper()",
        "configure_agent_build_environment()",
        "zodex-agent-build-env.sh",
        "export TMPDIR=",
        "export GOCACHE=",
        "export GOMODCACHE=",
        "export npm_config_cache=",
        "export BUN_INSTALL_CACHE_DIR=",
        "export COREPACK_HOME=",
        "export CCACHE_DIR=",
        "export PIP_CACHE_DIR=",
        "export UV_CACHE_DIR=",
        "keep site-specific toolchain policy in a separate profile fragment",
        "if [ -d /.sprite/bin ]",
        "export PATH=\"/.sprite/bin:\\${PATH}\"",
        "git config --global user.name",
        "git config --global user.email",
        "${ZODEX_STATE_DIR}/publisher/run",
        "${ZODEX_STATE_DIR}/publisher/logs",
        "credential.https://github.com.helper",
        "git-credential-helper",
        "git-remote-zodex",
        "zodex-agent",
        "print_runtime_summary()",
        "apt-get install -y --no-install-recommends",
        "build-essential pkg-config libssl-dev git",
        "zodex-prd",
        "service_port = ${ZODEX_SERVICE_PORT}",
        "agent_home = \"${ZODEX_AGENT_HOME}\"",
        "default_workdir = \"${ZODEX_DEFAULT_WORKDIR}\"",
        "Most installs can keep the built-in defaults.",
        "reader_app_id",
        "reader_installation_id",
        "publisher_client_id",
        "# id = \"amxv/zodex\"",
        "# repo = \"amxv/zodex\"",
        "credential.https://github.com.useHttpPath true",
        "url.\"zodex::https://github.com/\".pushInsteadOf",
        "Runtime lifecycle is managed from the operator machine with zodex sprite commands.",
    ];

    for snippet in required_snippets {
        assert!(
            script.contains(snippet),
            "install script missing snippet: {snippet}"
        );
    }
    assert!(
        script
            .lines()
            .all(|line| !line.trim_start().starts_with("rm -")),
        "installer cleanup must bypass user PATH wrappers with /bin/rm"
    );
    for removed in [
        "ZODEX_INSTALL_OPERATOR_CLI",
        "ZODEX_PUBLIC_HOST",
        "run_cli_install()",
        "resolved_public_host()",
        "print_next_steps()",
        "certbot",
        "tls_cert_path",
        "tls_key_path",
    ] {
        assert!(
            !script.contains(removed),
            "legacy runtime installer surface still present: {removed}"
        );
    }
}

#[test]
fn installer_auto_mode_is_always_operator_and_runtime_is_explicit() {
    let command = r#"
eval "$(sed -n '/^resolved_install_mode()/,/^}/p' "${INSTALL_SCRIPT}")"
ZODEX_INSTALL_MODE=auto
printf 'auto=%s\n' "$(resolved_install_mode)"
ZODEX_INSTALL_MODE=operator
printf 'operator=%s\n' "$(resolved_install_mode)"
ZODEX_INSTALL_MODE=runtime
printf 'runtime=%s\n' "$(resolved_install_mode)"
"#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .env("INSTALL_SCRIPT", install_script_path())
        .output()
        .expect("resolve installer modes");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "auto=operator\noperator=operator\nruntime=runtime\n"
    );
}

#[test]
fn runtime_binary_install_never_installs_operator_cli() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    let test_dir = std::env::temp_dir().join(format!(
        "zodex-runtime-install-test-{}-{unique}",
        std::process::id()
    ));
    let source_dir = test_dir.join("source");
    let install_dir = test_dir.join("install");
    std::fs::create_dir_all(&source_dir).expect("create source directory");
    std::fs::create_dir_all(&install_dir).expect("create install directory");

    for binary in [
        "zodex",
        "zodex-agent",
        "git-remote-zodex",
        "zodexd",
        "zodex-prd",
    ] {
        let path = source_dir.join(binary);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write fixture binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&path)
                .expect("read fixture metadata")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).expect("make fixture executable");
        }
    }
    std::fs::write(install_dir.join("zodex"), "stale operator\n")
        .expect("write stale operator binary");

    let command = r#"
eval "$(sed -n '/^install_binaries_from_dir()/,/^}/p' "${INSTALL_SCRIPT}")"
die() { printf '%s\n' "$*" >&2; exit 1; }
install_binaries_from_dir "${SOURCE_DIR}"
"#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .env("INSTALL_SCRIPT", install_script_path())
        .env("SOURCE_DIR", &source_dir)
        .env("ZODEX_INSTALL_DIR", &install_dir)
        .output()
        .expect("install runtime fixture binaries");
    if !output.status.success() {
        panic!(
            "runtime fixture install failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert!(!install_dir.join("zodex").exists());
    for binary in ["zodex-agent", "git-remote-zodex", "zodexd", "zodex-prd"] {
        assert!(install_dir.join(binary).is_file(), "missing {binary}");
    }

    std::fs::remove_dir_all(&test_dir).expect("remove runtime install test directory");
}

#[test]
fn install_script_does_not_use_generic_target_triple_tarball_match() {
    let script = std::fs::read_to_string(install_script_path()).expect("read install script");

    assert!(
        !script.contains("${TARGET_TRIPLE}[^\"]*\\.tar\\.gz"),
        "install script should not select release assets via generic target triple tarball match"
    );
    let deprecated_platform_name = ["run", "pod"].concat();
    assert!(
        !script.contains(&deprecated_platform_name),
        "install script should not contain deprecated platform-specific branches"
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

#[test]
fn operator_install_falls_back_to_user_local_bin_without_root() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    let test_dir = std::env::temp_dir().join(format!(
        "zodex-install-test-{}-{unique}",
        std::process::id()
    ));
    let source_dir = test_dir.join("source");
    let home_dir = test_dir.join("home");
    std::fs::create_dir_all(&source_dir).expect("create source directory");
    std::fs::create_dir_all(&home_dir).expect("create home directory");

    let source_binary = source_dir.join("zodex");
    std::fs::write(&source_binary, "#!/bin/sh\nexit 0\n").expect("write source binary");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&source_binary)
            .expect("read source binary metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&source_binary, permissions)
            .expect("make source binary executable");
    }

    let script_path = install_script_path();
    let command = r#"
eval "$(sed -n '/^operator_local_runtime_dir()/,/^}/p' "${INSTALL_SCRIPT}")"
eval "$(sed -n '/^ensure_local_stopped_before_operator_replace()/,/^}/p' "${INSTALL_SCRIPT}")"
eval "$(sed -n '/^install_operator_binary_atomically()/,/^}/p' "${INSTALL_SCRIPT}")"
eval "$(sed -n '/^install_operator_binaries_from_dir()/,/^}/p' "${INSTALL_SCRIPT}")"
is_root() { return 1; }
log() { printf '%s\n' "$*"; }
die() { printf '%s\n' "$*" >&2; exit 1; }
install_operator_binaries_from_dir "${SOURCE_DIR}"
"#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .env("INSTALL_SCRIPT", &script_path)
        .env("SOURCE_DIR", &source_dir)
        .env("HOME", &home_dir)
        .env("ZODEX_INSTALL_DIR", "/usr/local/bin")
        .output()
        .expect("run operator install");

    let installed_binary = home_dir.join(".local/bin/zodex");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("operator install failed: {stderr}");
    }
    assert!(
        installed_binary.is_file(),
        "operator install should fall back to {}",
        installed_binary.display()
    );

    std::fs::remove_dir_all(&test_dir).expect("remove test directory");
}

#[test]
fn operator_update_refuses_to_replace_zodex_while_local_runtime_state_exists() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    let test_dir = std::env::temp_dir().join(format!(
        "zodex-local-update-guard-test-{}-{unique}",
        std::process::id()
    ));
    let source_dir = test_dir.join("source");
    let install_dir = test_dir.join("install");
    let state_home = test_dir.join("state");
    let runtime_dir = state_home.join("zodex/local/runtime");
    std::fs::create_dir_all(&source_dir).expect("create source directory");
    std::fs::create_dir_all(&install_dir).expect("create install directory");
    std::fs::create_dir_all(&runtime_dir).expect("create Local runtime directory");

    let source_binary = source_dir.join("zodex");
    let installed_binary = install_dir.join("zodex");
    std::fs::write(&source_binary, "#!/bin/sh\necho new\n").expect("write replacement binary");
    std::fs::write(&installed_binary, "#!/bin/sh\necho old\n").expect("write installed binary");
    std::fs::write(runtime_dir.join("state.json"), "{}\n").expect("write runtime marker");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&source_binary, &installed_binary] {
            let mut permissions = std::fs::metadata(path)
                .expect("read binary metadata")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).expect("make binary executable");
        }
    }

    let script_path = install_script_path();
    let command = r#"
eval "$(sed -n '/^operator_local_runtime_dir()/,/^}/p' "${INSTALL_SCRIPT}")"
eval "$(sed -n '/^ensure_local_stopped_before_operator_replace()/,/^}/p' "${INSTALL_SCRIPT}")"
eval "$(sed -n '/^install_operator_binary_atomically()/,/^}/p' "${INSTALL_SCRIPT}")"
eval "$(sed -n '/^install_operator_binaries_from_dir()/,/^}/p' "${INSTALL_SCRIPT}")"
uname() { printf 'Darwin\n'; }
is_root() { return 0; }
log() { printf '%s\n' "$*"; }
die() { printf '%s\n' "$*" >&2; exit 1; }
install_operator_binaries_from_dir "${SOURCE_DIR}"
"#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .env("INSTALL_SCRIPT", &script_path)
        .env("SOURCE_DIR", &source_dir)
        .env("ZODEX_INSTALL_DIR", &install_dir)
        .env("XDG_STATE_HOME", &state_home)
        .env("HOME", test_dir.join("home"))
        .output()
        .expect("run guarded operator update");

    assert!(!output.status.success(), "active Local update must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("run 'zodex local stop' before upgrading"));
    assert_eq!(
        std::fs::read_to_string(&installed_binary).expect("read preserved installed binary"),
        "#!/bin/sh\necho old\n"
    );

    std::fs::remove_dir_all(&test_dir).expect("remove update guard test directory");
}

#[test]
fn atomic_operator_replacement_preserves_existing_binary_if_rename_fails() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    let test_dir = std::env::temp_dir().join(format!(
        "zodex-atomic-install-test-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&test_dir).expect("create atomic install test directory");
    let source = test_dir.join("new-zodex");
    let destination = test_dir.join("zodex");
    std::fs::write(&source, "new\n").expect("write source");
    std::fs::write(&destination, "old\n").expect("write destination");

    let script_path = install_script_path();
    let command = r#"
eval "$(sed -n '/^install_operator_binary_atomically()/,/^}/p' "${INSTALL_SCRIPT}")"
mv() { return 1; }
if install_operator_binary_atomically "${SOURCE}" "${DESTINATION}"; then
  exit 9
fi
"#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .env("INSTALL_SCRIPT", &script_path)
        .env("SOURCE", &source)
        .env("DESTINATION", &destination)
        .output()
        .expect("run atomic replacement failure fixture");
    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(&destination).expect("read preserved destination"),
        "old\n"
    );
    assert_eq!(
        std::fs::read_dir(&test_dir)
            .expect("list atomic install directory")
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".zodex-install."))
            .count(),
        0,
        "failed replacement must clean temporary install files"
    );

    std::fs::remove_dir_all(&test_dir).expect("remove atomic install test directory");
}

#[test]
fn operator_release_dir_install_proves_local_needs_only_zodex_binary() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    let test_dir = std::env::temp_dir().join(format!(
        "zodex-local-install-test-{}-{unique}",
        std::process::id()
    ));
    let release_dir = test_dir.join("zodex-test-release");
    let install_dir = test_dir.join("install");
    std::fs::create_dir_all(&release_dir).expect("create release directory");
    let real_zodex = env!("CARGO_BIN_EXE_zodex");
    assert!(
        !real_zodex.contains('\''),
        "test binary path unexpectedly contains a shell quote"
    );
    let fixture_zodex = release_dir.join("zodex");
    std::fs::write(
        &fixture_zodex,
        format!("#!/bin/sh\nexec '{real_zodex}' \"$@\"\n"),
    )
    .expect("write thin operator release fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&fixture_zodex)
            .expect("read fixture metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fixture_zodex, permissions).expect("make fixture executable");
    }

    let archive = test_dir.join("zodex-test-release.tar.gz");
    let tar_output = Command::new("tar")
        .args(["-C", test_dir.to_str().expect("utf-8 test path"), "-czf"])
        .arg(&archive)
        .arg(release_dir.file_name().expect("release directory name"))
        .output()
        .expect("create operator release archive");
    assert!(
        tar_output.status.success(),
        "tar failed: {}",
        String::from_utf8_lossy(&tar_output.stderr)
    );

    let archive_bytes = std::fs::read(&archive).expect("read operator release archive");
    let digest = Sha256::digest(&archive_bytes);
    std::fs::write(
        format!("{}.sha256", archive.display()),
        format!(
            "{digest:x}  {}\n",
            archive.file_name().unwrap().to_string_lossy()
        ),
    )
    .expect("write operator release checksum");
    let asset_url = format!("file://{}", archive.display());

    let output = Command::new("bash")
        .arg(install_script_path())
        .env("ZODEX_ASSET_URL", &asset_url)
        .env("ZODEX_INSTALL_DIR", &install_dir)
        .output()
        .expect("run operator release-dir install");
    if !output.status.success() {
        panic!(
            "operator install failed\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let installed = install_dir.join("zodex");
    assert!(installed.is_file());
    for absent in ["zodex-client", "zodex-agent", "zodexd", "zodex-prd"] {
        assert!(!install_dir.join(absent).exists(), "unexpected {absent}");
    }

    let help = Command::new(&installed)
        .args(["local", "--help"])
        .output()
        .expect("run installed Local help");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage: zodex local <COMMAND>"));

    let setup_help = Command::new(&installed)
        .args(["local", "setup", "--help"])
        .output()
        .expect("run installed Local setup help");
    assert!(setup_help.status.success());
    assert!(String::from_utf8_lossy(&setup_help.stdout).contains("--runtime-key-stdin"));

    std::fs::remove_dir_all(&test_dir).expect("remove Local install test directory");
}
