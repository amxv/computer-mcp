#[test]
fn cargo_manifest_exposes_zodex_binary_names() {
    let manifest = include_str!("../Cargo.toml");

    assert!(manifest.contains("name = \"zodex\""));
    assert!(manifest.contains("name = \"zodex-agent\""));
    assert!(manifest.contains("name = \"git-remote-zodex\""));
    assert!(manifest.contains("name = \"zodex-client\""));
    assert!(manifest.contains("name = \"zodexd\""));
    assert!(manifest.contains("name = \"zodex-prd\""));
}

#[test]
fn release_and_source_manifest_keep_local_inside_the_single_macos_operator_artifact() {
    let release = include_str!("../.github/workflows/release.yml");
    assert!(release.contains("target: aarch64-apple-darwin"));
    assert!(release.contains("target: x86_64-apple-darwin"));
    assert!(release.contains("--bin zodex"));
    assert!(release.contains("target/${{ matrix.target }}/release/zodex"));

    let provider = include_str!("../src/bin/zodex/local_provider.rs");
    let setup = include_str!("../src/bin/zodex/local_setup.rs");
    let network = include_str!("../src/bin/zodex/local_network.rs");
    let tunnel = include_str!("../src/bin/zodex/local_tunnel.rs");

    assert!(provider.contains("include_str!(\"local_machine.Containerfile\")"));
    assert!(setup.contains("include_str!(\"local_zodexd.service\")"));
    assert!(setup.contains("include_str!(\"local_zodex_prd.service\")"));
    assert!(network.contains("include_str!(\"local_agent_network.sh\")"));
    assert!(network.contains("include_str!(\"local_agent_network.service\")"));
    assert!(tunnel.contains("include_str!(\"local_zodex_tunnel.service\")"));

    for source in [provider, setup, network, tunnel] {
        assert!(!source.contains("CARGO_MANIFEST_DIR"));
    }
}
