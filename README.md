# zodex

Zodex puts ChatGPT on a real coding machine through three familiar MCP tools:

- `exec_command`
- `write_stdin`
- `apply_patch`

The same tool surface works across two first-class execution modes.

| Mode | Machine | Trust model | Connection |
| --- | --- | --- | --- |
| **Local** | Your trusted Apple Silicon Mac | Direct execution as your logged-in user; no Zodex sandbox | OpenAI Secure MCP Tunnel |
| **Sprite** | Remote Sprite-backed Linux | Isolated remote workspace with explicit GitHub write controls | Public HTTPS MCP endpoint |

Local is for the Mac where your code, toolchains, files, and credentials already live. Sprite is for persistent remote Linux workspaces with a separate GitHub autonomy boundary.

## Install

```bash
curl -fsSL https://zodex.ashray.xyz/install.sh | sh
zodex --version
```

## Local

Set up the tunnel once, then start Zodex from the workspace you want ChatGPT to begin with:

```bash
zodex local setup
cd ~/code/my-project
zodex local start --ttl 4h
zodex local watch
```

One Local runtime can serve multiple ChatGPT conversations. It also includes durable Agent-aware history, a first-party watch TUI, and a read-only localhost HTTP/SSE observability API for custom clients.

[Local documentation →](https://zodex.ashray.xyz/docs/local)

## Sprite

Create a remote Sprite, configure the reader/writer GitHub Apps, then let Zodex provision the runtime. The exact setup flags are documented in the Sprite quick start; after setup, normal operator commands are short:

```bash
zodex sprite status --sprite zodex-dev
zodex sprite health --sprite zodex-dev
```

Sprite supports review-first PR publishing, temporary repo-scoped push grants, operator-granted write windows, and scoped/timed YOLO mode without exposing the writer App credentials to the normal agent shell.

[Sprite documentation →](https://zodex.ashray.xyz/docs/sprite)

## Documentation

The docs site is the source of truth for setup, security boundaries, commands, observability, GitHub write modes, operations, and troubleshooting:

- [Documentation](https://zodex.ashray.xyz/docs)
- [Local](https://zodex.ashray.xyz/docs/local)
- [Sprite](https://zodex.ashray.xyz/docs/sprite)
- [Shared reference](https://zodex.ashray.xyz/docs/reference/architecture)

## Development

```bash
cargo test --quiet
bun run check
bun run build
```
