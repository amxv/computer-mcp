use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|error| panic!("read {path}: {error}"))
}

#[test]
fn sprite_site_guide_describes_current_first_class_setup_flow() {
    let setup = read("src/content/docs/sprite.md");

    for required in [
        "zodex sprite setup",
        "curl -fsSL https://zodex.ashray.xyz/install.sh | sh",
        "curl -fsSL https://sprites.dev/install.sh | sh",
        "sprite login",
        "sprite create zodex-dev",
        "sprite info zodex-dev",
        "--reader-app-id",
        "--reader-pem",
        "--publisher-app-id",
        "--publisher-client-id",
        "--publisher-pem",
        "Cloudflare Worker",
        "claim URL",
        "60 minutes",
        "wrangler login --use-keyring",
        "zodex sprite proxy deploy",
        "zodex sprite connect",
        "zodex-agent github request-push",
        "zodex sprite github grant-push",
        "zodex-agent github publish-pr",
    ] {
        assert!(
            setup.contains(required),
            "Sprite guide missing `{required}`"
        );
    }

    for removed in [
        "sprite url update",
        "npx wrangler deploy",
        "zodex github ",
        "zodex proxy ",
        "--url-auth sprite",
        "zodex-client",
    ] {
        assert!(
            !setup.contains(removed),
            "Sprite guide still contains legacy surface `{removed}`"
        );
    }

    assert!(!repo_root().join("docs/setup.md").exists());
}
