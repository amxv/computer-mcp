use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_path(path: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn read(path: &str) -> String {
    fs::read_to_string(repo_path(path)).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn sprite_public_docs() -> String {
    [
        "README.md",
        "AGENTS.md",
        ".agents/skills/sprites/SKILL.md",
        "src/content/docs/sprite.md",
        "src/content/docs/sprite/connect.md",
        "src/content/docs/sprite/operations.md",
        "src/content/docs/sprite/configuration.md",
        "src/content/docs/sprite/github-apps.md",
        "src/content/docs/sprite/permissions.md",
        "src/content/docs/sprite/write-modes.md",
        "src/content/docs/sprite/push-grants.md",
        "src/content/docs/sprite/operator-controls.md",
        "src/content/docs/sprite/command-reference.md",
        "src/content/docs/sprite/troubleshooting.md",
        "src/content/docs/reference/architecture.md",
        "src/pages/index.astro",
    ]
    .into_iter()
    .map(read)
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn public_positioning_exposes_local_and_sprite_as_first_class_modes() {
    let readme = read("README.md");
    let site = read("src/data/docs.ts");
    let home = read("src/pages/index.astro");

    for required in [
        "two first-class execution modes",
        "OpenAI Secure MCP Tunnel",
        "Canonical Cloudflare Worker",
        "Wake-on-demand remote Linux",
        "zodex local start",
        "zodex sprite setup --help",
    ] {
        assert!(readme.contains(required), "README missing `{required}`");
    }
    assert!(site.contains(
        "export const docCategories = [\n  \"Local\",\n  \"Sprite\",\n  \"Reference\"\n] as const;"
    ));
    assert!(home.contains("href: \"/docs/local\""));
    assert!(home.contains("href: \"/docs/sprite\""));
    assert!(home.contains("canonical Cloudflare Worker"));
    assert!(home.contains("sprite info zodex-dev"));
}

#[test]
fn local_guide_preserves_trusted_host_tunnel_and_current_chatgpt_prerequisite() {
    let guide = read("src/content/docs/local.md");
    let setup = read("src/content/docs/local/setup.md");
    for required in [
        "trusted Apple Silicon Mac",
        "not a sandbox",
        "OpenAI Secure MCP Tunnel",
        "suggested initial explicit workdir",
        "does not edit TCC databases",
        "zodex local watch --agent k7m2",
        "zodex local history --agent k7m2 --since 1h",
    ] {
        assert!(guide.contains(required), "Local guide missing `{required}`");
    }
    for required in [
        "Most OpenAI paid plans support custom MCP servers",
        "curl -fsSL https://zodex.ashray.xyz/install.sh | sh",
    ] {
        assert!(
            setup.contains(required) || guide.contains(required),
            "Local docs missing `{required}`"
        );
    }
    assert!(!guide.contains("workdir fallback"));
}

#[test]
fn sprite_quick_start_documents_canonical_worker_and_one_setup_command() {
    let guide = read("src/content/docs/sprite.md");
    for required in [
        "Cloudflare Worker",
        "wake-on-demand remote Linux",
        "sprite login",
        "sprite create zodex-dev",
        "sprite info zodex-dev",
        "--reader-app-id <reader-app-id>",
        "--reader-pem /absolute/path/to/reader.pem",
        "--publisher-app-id <writer-app-id>",
        "--publisher-client-id <writer-client-id>",
        "--publisher-pem /absolute/path/to/writer.pem",
        "temporary",
        "claim URL",
        "60 minutes",
        "wrangler login --use-keyring",
        "zodex sprite proxy deploy --sprite zodex-dev",
        "--cloudflare-account <id-or-name>",
        "zodex sprite connect --sprite zodex-dev",
        "Most OpenAI paid plans support custom MCP servers",
        "start the Sprite manually",
    ] {
        assert!(
            guide.contains(required),
            "Sprite guide missing `{required}`"
        );
    }
    assert!(guide.contains("Do not clone Zodex or edit a Wrangler file"));
}

#[test]
fn sprite_docs_use_current_namespaces_and_remove_dead_public_surfaces() {
    let docs = sprite_public_docs();
    for required in [
        "zodex sprite proxy deploy",
        "zodex sprite proxy status",
        "zodex sprite proxy verify",
        "zodex sprite github grant-push",
        "zodex sprite github revoke-push",
        "zodex sprite github yolo",
        "zodex sprite github default",
        "sprite info",
        "sprite config update --url-auth public",
        "zodex-agent github publish-pr",
        "zodex-agent github request-push",
    ] {
        assert!(docs.contains(required), "current docs missing `{required}`");
    }

    for removed in [
        "zodex proxy ",
        "zodex github ",
        "zodex github mode",
        "sprite url update",
        "npx wrangler deploy",
        "ZODEX_INSTALL_MODE=operator",
        "zodex-client",
        "/v1/exec-command",
        "/v1/write-stdin",
        "/v1/apply-patch",
        "--url-auth sprite",
    ] {
        assert!(
            !docs.contains(removed),
            "legacy public Sprite surface still documented: `{removed}`"
        );
    }

    assert!(!repo_path("docs/setup.md").exists());
    assert!(!repo_path("src/content/docs/sprite/http-api.md").exists());
}

#[test]
fn sprite_github_app_docs_define_exact_user_owned_security_boundary() {
    let docs = read("src/content/docs/sprite/github-apps.md");
    for required in [
        "Create and install both Apps yourself",
        "Contents: Read-only",
        "Contents: Read & write",
        "Pull requests: Read & write",
        "Workflows: Read & write",
        "Device Flow",
        "Only select repositories",
        "App ID",
        "Client ID",
        "private-key.pem",
        "zodex-publisher",
        "must **not** be able to read the writer PEM",
    ] {
        assert!(
            docs.contains(required),
            "GitHub App docs missing `{required}`"
        );
    }
}

#[test]
fn sprite_configuration_describes_plain_http_service_and_128_mib_limit() {
    let configuration = read("src/content/docs/sprite/configuration.md");
    for required in [
        "service_port = 8080",
        "plain HTTP behind the Sprite HTTPS edge",
        "128 MiB",
        "requires explicit absolute `workdir` values",
        "sprite config update --url-auth public dev",
    ] {
        assert!(
            configuration.contains(required),
            "Sprite configuration missing `{required}`"
        );
    }
    for removed in [
        "tls_mode",
        "tls_cert_path",
        "tls_key_path",
        "http_bind_port",
    ] {
        assert!(
            !configuration.contains(removed),
            "dead runtime config field still documented: `{removed}`"
        );
    }
}

#[test]
fn sprite_connect_documents_capability_url_and_at_most_once_dispatch() {
    let connect = read("src/content/docs/sprite/connect.md");
    for required in [
        "canonical Cloudflare Worker",
        "at most once",
        "credential-in-URL leakage",
        "Treat the entire endpoint as a secret",
        "does not document a static custom-header/API-key field",
        "zodex sprite connect --sprite dev",
    ] {
        assert!(
            connect.contains(required),
            "connect guide missing `{required}`"
        );
    }
}

#[test]
fn sprite_default_policy_is_documented_as_independent_from_explicit_grants() {
    for path in [
        "src/content/docs/sprite/permissions.md",
        "src/content/docs/sprite/write-modes.md",
        "src/content/docs/sprite/operator-controls.md",
    ] {
        let docs = read(path);
        assert!(docs.contains("grant"));
        assert!(docs.contains("default"));
        assert!(
            docs.contains("does **not**")
                || docs.contains("does not")
                || docs.contains("remain independent")
                || docs.contains("remain until")
        );
    }
}

#[test]
fn local_watch_client_guide_matches_read_only_observer_surface() {
    let guide = read("src/content/docs/local/observability-api.md");
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
        "recovery_since_ms",
        "event_type: \"gap\"",
        "runtime_id",
        "presentation_version",
        "examples/local_observability_client.py",
    ] {
        assert!(
            guide.contains(required),
            "observer guide missing `{required}`"
        );
    }
}

#[test]
fn architecture_keeps_shared_stateless_mcp_and_distinct_mode_credentials() {
    let architecture = read("src/content/docs/reference/architecture.md");
    for required in [
        "exec_command",
        "write_stdin",
        "apply_patch",
        "explicit absolute existing `workdir`",
        "stateless MCP requests",
        "OpenAI Secure MCP Tunnel",
        "Cloudflare Worker",
        "at most once",
        "zodex-publisher",
        "default` removes YOLO policy without deleting unrelated explicit grants",
    ] {
        assert!(
            architecture.contains(required),
            "architecture missing `{required}`"
        );
    }
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
