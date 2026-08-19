# zodex

Give ChatGPT a real machine through three familiar MCP tools:

- `exec_command`
- `write_stdin`
- `apply_patch`

Zodex supports two execution modes with the same agent-facing tool surface:

- **Local** — run ChatGPT directly on your trusted Apple Silicon Mac through an OpenAI Secure MCP Tunnel.
- **Sprite** — run ChatGPT in a wake-on-demand remote Linux workspace with scoped GitHub write controls.

## Install

Install the Zodex operator CLI on Apple Silicon macOS or Linux (x86_64/aarch64):

```bash
curl -fsSL https://zodex.ashray.xyz/install.sh | bash
export PATH="$HOME/.local/bin:$PATH"
```

Then choose a mode:

```bash
# Local
zodex local setup
zodex local start --ttl 4h

# Sprite
zodex sprite setup --help
```

## Documentation

Read the full docs at **[zodex.ashray.xyz/docs](https://zodex.ashray.xyz/docs)**.

- [Local](https://zodex.ashray.xyz/docs/local) — setup, daily use, Liveboard, configuration, and troubleshooting
- [Sprite](https://zodex.ashray.xyz/docs/sprite) — setup, GitHub permissions, connection, operations, and troubleshooting
- [Reference](https://zodex.ashray.xyz/docs/reference/architecture) — architecture, MCP tools, development, and changelog
