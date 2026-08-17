---
title: "Development"
description: "Maintainer reference for building, testing, and releasing Zodex itself; not required to operate Sprite or Local."
order: 3
category: Reference
summary: "Repository-maintainer checks for runtime, CLI, scripts, releases, and docs."
---

## Rust checks

Run the full Rust test suite:

```bash
cargo test --quiet
```

For CLI behavior changes, also inspect help output:

```bash
cargo run --quiet --bin zodex -- --help
cargo run --quiet --bin zodex -- sprite --help
cargo run --quiet --bin zodex -- proxy --help
cargo run --quiet --bin zodex -- github --help
cargo run --quiet --bin zodex-agent -- --help
cargo run --quiet --bin zodex-agent -- github publish-pr --help
```

The tests cover binary manifests, CLI behavior, GitHub App scripts, install behavior, Sprite scripts, zodex-agent forwarding, MCP tool registration, session handling, redaction, patch application, and mode-first product contracts.

## Liveboard checks

Liveboard is an isolated frontend package under `web/liveboard`. On macOS its production assets are embedded into the `zodex` binary by `build.rs`; Linux/Sprite builds do not require frontend tooling.

```bash
cd web/liveboard
bun install --frozen-lockfile
bunx playwright install chromium webkit
bun run typecheck
bun run test
bun run test:browser
bun run test:browser:webkit
bun run build
```

Do not commit `web/liveboard/node_modules/` or `web/liveboard/dist/`.

For an embed-required macOS validation, build the frontend first and then run Cargo with:

```bash
ZODEX_LIVEBOARD_EMBED_REQUIRED=1 cargo test
```

CI does this only on the native macOS lane. Release builds likewise install Bun/build Liveboard only for the Apple target; Linux/Sprite release targets stay independent of the frontend toolchain.

## Docs site checks

Run:

```bash
bun install
bun run check
bun run build
```

Do not commit generated Astro output:

```text
.astro/
dist/
node_modules/
```

These paths are ignored.

## Docs content rules

Keep docs tied to actual zodex behavior:

- mention the real binaries: `zodex`, `zodex-agent`, `git-remote-zodex`, `zodexd`, `zodex-prd`
- distinguish operator-machine commands from Sprite-side commands
- keep the read/write access model explicit
- explain when a command needs an active grant
- keep MCP as the supported remote coding transport; do not reintroduce deleted legacy transports
- update command examples when Clap arguments change
- when Local observability routes, response fields, filters, SSE event types, discovery fields, API/presentation versions, or recovery semantics change, update [Local observability API](/docs/local/observability-api) in the same change
- when Liveboard/TUI controls, board behavior, presentation, or recovery UX changes, update [Watch and Liveboard](/docs/local/watch) in the same change

## Repository scripts

Useful scripts include:

```bash
scripts/install.sh
scripts/mint-gh-app-installation-token.sh
scripts/protect-main-branch.sh
scripts/github_actions_fail_fast.py
```

Run script-specific tests when changing them:

```bash
cargo test --quiet --test install_script
cargo test --quiet --test github_app_scripts
```

## Release awareness

The crate version is defined in `Cargo.toml`. The repository uses tagged releases.

When a release changes CLI arguments, binary names, setup behavior, service layout, Liveboard assets, or public observer contracts, update the docs site in the same change.
