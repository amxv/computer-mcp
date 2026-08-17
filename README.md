# zodex

Give ChatGPT a real machine through three familiar MCP tools:

- `exec_command`
- `write_stdin`
- `apply_patch`

Zodex has two first-class execution modes. They share the same tool contract and explicit absolute `workdir` model, but deliberately use different host and trust boundaries.

| Mode | Host | Trust model | ChatGPT connection |
| --- | --- | --- | --- |
| **Local** | Your trusted Apple Silicon Mac | Commands run as your logged-in user; Zodex is not a sandbox | OpenAI Secure MCP Tunnel |
| **Sprite** | Wake-on-demand remote Linux | Restricted guest runtime plus isolated GitHub writer credentials | Canonical Cloudflare Worker → public Sprite wake edge |

## Install

Install the operator CLI on macOS or Linux:

```bash
curl -fsSL https://zodex.ashray.xyz/install.sh | sh
```

Then choose a mode:

```bash
# Trusted Apple Silicon Mac
zodex local setup
zodex local start --ttl 4h

# Wake-on-demand remote Linux
zodex sprite setup --help
```

The normal installer always installs the operator CLI. Sprite setup provisions the smaller guest runtime explicitly; the operator `zodex` binary is not installed in the agent shell.

## Local

Local is for a Mac you already trust. It connects ChatGPT through an OpenAI Secure MCP Tunnel, runs commands as your logged-in user, and includes Agent-aware live observation plus durable history.

Start here: [Local guide](https://zodex.ashray.xyz/docs/local)

## Sprite

Sprite is for a separate Linux workspace that can sleep when idle and wake on incoming work. The supported ChatGPT path is:

```text
ChatGPT custom app
  → Cloudflare Worker
  → public Sprite HTTPS wake edge
  → zodexd
  → restricted zodex-agent workspace
```

The Worker is deployed by the installed operator; users do not need a Zodex checkout or hand-edited Wrangler project. GitHub reads use a narrow reader App, while PR publishing and direct-push policy use a separate writer App whose private key stays isolated from the agent account.

Start here: [Sprite guide](https://zodex.ashray.xyz/docs/sprite)

## ChatGPT availability

OpenAI's current custom-app guidance documents full write/modify MCP for **ChatGPT Business and Enterprise/Edu on ChatGPT web**. **Pro custom MCP remains read/fetch-only.** Check the current OpenAI guidance before setup because plan and workspace availability can change: [Developer mode and MCP apps in ChatGPT](https://help.openai.com/en/articles/12584461-developer-mode-and-mcp-apps-in-chatgpt).

## Why only three tools?

Frontier GPT models already know how to work with a command runner, an interactive stdin channel, and patch application. Zodex keeps that surface small while handling PTYs, process groups, long-running sessions, explicit working directories, runtime cleanup, and mode-specific infrastructure underneath it.

## Documentation

- [Local](https://zodex.ashray.xyz/docs/local)
- [Sprite](https://zodex.ashray.xyz/docs/sprite)
- [Architecture](https://zodex.ashray.xyz/docs/reference/architecture)
- [Tool reference](https://zodex.ashray.xyz/docs/reference/tools)
