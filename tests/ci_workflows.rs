use std::fs;
use std::path::{Path, PathBuf};

fn repo_path(path: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn read(path: &str) -> String {
    fs::read_to_string(repo_path(path)).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

#[test]
fn normal_ci_covers_linux_and_native_apple_silicon_without_secrets() {
    let workflow = read(".github/workflows/ci.yml");

    for required in [
        "push:",
        "pull_request:",
        "workflow_dispatch:",
        "ubuntu-latest",
        "runs-on: macos-15",
        "CARGO_BUILD_TARGET: aarch64-apple-darwin",
        "targets: aarch64-apple-darwin",
        "gg/zodex-local/zodex-local-implementation-plan-2026-08-15.md",
        "gg/zodex-local/zodex-local-acceptance.md",
        "bash scripts/check-local-contract.sh",
        "bash scripts/check.sh",
    ] {
        assert!(
            workflow.contains(required),
            "CI workflow missing `{required}`"
        );
    }
    assert!(workflow.contains("branches:\n      - main\n      - zodex-local"));
    assert!(
        !workflow.contains("secrets."),
        "ordinary CI must not require provider/private credentials"
    );
}

#[test]
fn portable_contract_script_pins_modern_legacy_workdir_and_local_operator_proofs() {
    let script = read("scripts/check-local-contract.sh");
    for required in [
        "modern_stateless_tool_call_observes_openai_session_without_transport_session",
        "tunnel_compat_initialize_is_sessionless_and_has_no_provider_attribution",
        "workdir_is_required_in_model_visible_exec_and_patch_schemas",
        "local_discovery_and_tools_list_are_stateless_and_runtime_specific",
        "zodex_local_help_exposes_complete_public_family_and_inspection_examples",
        "zodex-local-implementation-plan-2026-08-15.md",
        "zodex-local-acceptance.md",
        "acceptance-map:start",
        "plan-acceptance-cksum",
        "plan-file-cksum",
        "cksum",
        "Phase-13-native",
        "/bin/rm -rf \"$tmpdir\"",
        "-- --exact",
    ] {
        assert!(
            script.contains(required),
            "portable contract check missing `{required}`"
        );
    }
    assert!(
        !script.contains("144"),
        "acceptance validation must derive the current criterion set rather than hard-code the planning-time count"
    );
}

#[test]
fn release_keeps_asset_names_and_smokes_local_from_operator_only_macos_install() {
    let workflow = read(".github/workflows/release.yml");

    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
    ] {
        assert!(workflow.contains(target), "release lost target {target}");
    }
    assert!(
        !workflow.contains("x86_64-apple-darwin"),
        "Intel macOS release support is intentionally out of scope"
    );
    for required in [
        "archive_name: zodex-aarch64-apple-darwin",
        "Smoke test packaged macOS Local operator",
        "ZODEX_INSTALL_MODE=operator",
        "ZODEX_ASSET_URL=\"file://${GITHUB_WORKSPACE}/dist/${{ matrix.archive_name }}.tar.gz\"",
        "test ! -e \"$install_dir/zodexd\"",
        "test ! -e \"$install_dir/zodex-agent\"",
        "\"$install_dir/zodex\" local --help",
        "\"$install_dir/zodex\" local setup --help",
        "must not bundle Local setup/runtime/private-state files",
    ] {
        assert!(
            workflow.contains(required),
            "release workflow missing `{required}`"
        );
    }
    assert!(workflow.contains("os: macos-15"));
    assert!(workflow.contains("targets: ${{ matrix.target }}"));
}

#[test]
fn macos_native_dependency_remains_target_scoped() {
    let manifest = read("Cargo.toml");
    let target_header = "[target.'cfg(target_os = \"macos\")'.dependencies]";
    let target_start = manifest
        .find(target_header)
        .expect("macOS target dependency section");
    let dev_start = manifest[target_start..]
        .find("[dev-dependencies]")
        .map(|offset| target_start + offset)
        .expect("dev dependency section after macOS dependencies");
    let macos_dependencies = &manifest[target_start..dev_start];

    assert!(macos_dependencies.contains("security-framework"));
    assert_eq!(
        manifest[..target_start]
            .matches("security-framework")
            .count(),
        0,
        "Keychain dependency must not leak into Linux/shared dependencies"
    );
}
