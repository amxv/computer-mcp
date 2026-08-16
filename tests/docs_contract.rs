use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_path(path: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn read(path: &str) -> String {
    fs::read_to_string(repo_path(path)).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

#[test]
fn public_positioning_exposes_both_local_and_sprite_modes() {
    let readme = read("README.md");
    let site = read("src/data/docs.ts");
    let home = read("src/pages/index.astro");

    for required in [
        "**Sprite**",
        "**Local**",
        "Apple Silicon",
        "Sprite-backed Linux",
        "zodex local start",
    ] {
        assert!(readme.contains(required), "README missing `{required}`");
    }
    assert!(site.contains("trusted direct Apple Silicon Mac execution"));
    assert!(site.contains("\"Local Clients\""));
    assert!(home.contains("href: \"/docs/local\""));
    assert!(home.contains("href: \"/docs/quickstart\""));
    assert!(home.contains("Same three tools."));
}

#[test]
fn local_guide_documents_current_trust_lifecycle_and_agent_contract() {
    let guide = read("src/content/docs/local.md");
    for required in [
        "trusted Apple Silicon Mac",
        "no Zodex confinement boundary",
        "not a sandbox",
        "One runtime-wide TTL",
        "There is exactly **one** TTL for the whole Local runtime",
        "suggested initial explicit workdir",
        "_meta[\"openai/session\"]",
        "four-character",
        "zodex local watch --agent k7m2",
        "zodex local history --agent k7m2 --since 1h",
        "zodex local history --workdir /absolute/repo/path",
        "zodex local history --id <invocation-id> --raw",
        "kill everything normally spawned by Zodex",
        "does not edit TCC databases",
    ] {
        assert!(guide.contains(required), "Local guide missing `{required}`");
    }
    assert!(
        !guide.contains("workdir fallback"),
        "Local guide must not describe an implicit workdir fallback"
    );
    assert!(
        !guide.contains("Phase-"),
        "public Local guide must not expose implementation-phase language"
    );
}

#[test]
fn configuration_no_longer_claims_mcp_default_workdir_fallback() {
    let configuration = read("src/content/docs/configuration.md");
    assert!(configuration.contains("It is **not** an MCP execution fallback"));
    assert!(configuration.contains("require an explicit absolute existing `workdir`"));
    assert!(!configuration.contains(
        "`exec_command` resolves its working directory from the tool input first, then from `default_workdir`"
    ));
}

#[test]
fn local_watch_client_guide_matches_read_only_observer_surface() {
    let guide = read("src/content/docs/local-watch-client.md");
    for route in [
        "GET /v1/status",
        "GET /v1/agents",
        "GET /v1/agents/{id}",
        "GET /v1/invocations",
        "GET /v1/invocations/{id}",
        "GET /v1/invocations/{id}/output",
        "GET /v1/events",
    ] {
        assert!(guide.contains(route), "observer guide missing `{route}`");
    }
    for required in [
        "Authorization: Bearer <token>",
        "Cache-Control: no-store",
        "Do **not** use unauthenticated `EventSource`",
        "same-origin localhost backend/wrapper",
        "recovery_since_ms",
        "event_type: \"gap\"",
        "next_cursor",
        "runtime_id",
        "presentation_version",
        "examples/local_observability_client.py",
        "should **not** parse `_meta[\"openai/session\"]`",
    ] {
        assert!(
            guide.contains(required),
            "observer guide missing `{required}`"
        );
    }
    assert!(repo_path("examples/local_observability_client.py").is_file());
}

#[test]
fn architecture_documents_modern_stateless_mcp_without_workdir_fallback() {
    let architecture = read("src/content/docs/architecture.md");
    for required in [
        "RMCP 3.x",
        "MCP `2026-07-28` stateless requests",
        "do not depend on transport-session state",
        "outside the model-visible tool arguments",
        "suggested initial explicit workdir",
        "X-Zodex-Local-Token",
        "macOS Keychain",
        "observability server uses its own automatically managed localhost bearer",
    ] {
        assert!(
            architecture.contains(required),
            "architecture missing `{required}`"
        );
    }
}

#[test]
fn sprite_proxy_and_write_policy_are_not_claimed_as_local_requirements() {
    let proxy = read("src/content/docs/proxy-mcp.md");
    let write_modes = read("src/content/docs/write-modes.md");
    let tools = read("src/content/docs/tools.md");
    assert!(proxy.contains("This page is **Sprite-specific**"));
    assert!(proxy.contains("Zodex Local does not require this Cloudflare/Sprite proxy"));
    assert!(write_modes.contains("These write modes are the **Sprite GitHub autonomy model**"));
    assert!(tools.contains("In **Local mode**, commands run as the trusted logged-in Mac user"));
}

#[test]
fn local_help_exposes_documented_agent_inspection_examples() {
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "--help"])
        .output()
        .expect("run Local help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "zodex local watch --agent k7m2",
        "zodex local history --agent k7m2 --since 1h",
        "zodex local history --workdir /absolute/repo/path",
        "zodex local history --id <invocation-id> --raw",
    ] {
        assert!(stdout.contains(expected), "Local help missing `{expected}`");
    }
}

#[test]
fn new_local_doc_routes_are_linked_from_public_surfaces() {
    let local = repo_path("src/content/docs/local.md");
    let client = repo_path("src/content/docs/local-watch-client.md");
    assert!(local.is_file());
    assert!(client.is_file());

    let home = read("src/pages/index.astro");
    let quickstart = read("src/content/docs/quickstart.md");
    let local_guide = read("src/content/docs/local.md");
    let troubleshooting = read("src/content/docs/troubleshooting.md");
    assert!(home.contains("/docs/local"));
    assert!(quickstart.contains("[Zodex Local](/docs/local)"));
    assert!(local_guide.contains("/docs/local-watch-client"));
    assert!(troubleshooting.contains("/docs/local-watch-client"));
}
