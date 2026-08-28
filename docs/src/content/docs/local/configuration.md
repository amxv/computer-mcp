---
title: "Configuration"
description: "Configure Zodex Local context injection, skill discovery, history retention, and tunnel metadata."
order: 6
category: Local
summary: "Control the Codex-style context ChatGPT receives, choose skill roots, configure retention and tunnel metadata, and know which changes are live."
---

Local has its own user-scoped configuration. It is separate from the `/etc/zodex/config.toml` used by Sprite servers.

Most users never edit the file directly. Use:

```bash
zodex local config get
```

## Read one setting

```bash
zodex local config get history.max-age
zodex local config get history.max-size
zodex local config get tunnel.id
```

## Change a setting

Context settings can be changed while Local is running:

```bash
zodex local config set context.enabled false
zodex local config set context.repo-agents false
zodex local config set context.skills.codex false
```

History and tunnel settings still require Local to be stopped:

```bash
zodex local stop
zodex local config set history.max-age 30d
zodex local config set history.max-size 1gb
```

If you try to change a non-context setting while Local is active, Zodex tells you to stop it first rather than creating a “current runtime versus next runtime” split.

`context.*` changes apply to future eligible results immediately. One-time context that has already been delivered, or a workdir check that has already been consumed, is not replayed just because you change a setting later.

## Automatic Codex-style context

By default, Zodex Local gives each ChatGPT conversation a small amount of host context alongside tool results. The command result, patch result, structured output, stdout, and status fields are left unchanged; Zodex adds context as a separate MCP text block after the real result has been produced.

The defaults are:

```toml
[context]
enabled = true
global_agents = true
repo_agents = true

[context.skills]
enabled = true
agents = true
codex = true
paths = []
```

### Global instructions

On the first Zodex tool result for a ChatGPT conversation, Local looks in `CODEX_HOME` for global instructions:

```text
$CODEX_HOME/AGENTS.override.md
$CODEX_HOME/AGENTS.md
```

`AGENTS.override.md` wins when both exist. The selected file's full text is appended to that first result. If neither file exists, nothing is added for global instructions.

`CODEX_HOME` comes from the developer environment captured by `zodex local start`. If it is unset, Local uses `$HOME/.codex`, matching Codex's normal default.

Disable this independently with:

```bash
zodex local config set context.global-agents false
```

### Global skills

The same first result also gets a compact catalog headed:

```text
Global skills on this machine:
```

Each entry contains only the skill's frontmatter `name`, `description`, and `SKILL.md` path. Zodex does not inject the skill body; the Agent can read the listed file when the skill is relevant.

By default Local mirrors Codex's two user-level skill sources:

```text
$HOME/.agents/skills
$CODEX_HOME/skills
```

It recursively discovers `SKILL.md` files up to six directories deep, follows directory symlinks, and includes Codex's `$CODEX_HOME/skills/.system` skills. Canonical paths are used to avoid listing the same symlinked skill twice, while the path from the configured discovery root is shown to ChatGPT.

Turn the entire skill catalog off:

```bash
zodex local config set context.skills.enabled false
```

Use only `$HOME/.agents/skills`:

```bash
zodex local config set context.skills.agents true
zodex local config set context.skills.codex false
```

Use only `$CODEX_HOME/skills`:

```bash
zodex local config set context.skills.agents false
zodex local config set context.skills.codex true
```

The default uses both. Additional skill roots are additive:

```bash
zodex local config set context.skills.paths '["~/team-skills", "/opt/company/skills"]'
```

Custom roots should be absolute paths or start with `~/`. To use only custom roots, disable both built-in sources and set `context.skills.paths`.

You can also edit the TOML directly:

```toml
[context.skills]
enabled = true
agents = false
codex = false
paths = ["~/team-skills", "/opt/company/skills"]
```

### Workdir `AGENTS` hints

For `exec_command` and `apply_patch`, Local also checks the exact absolute `workdir` supplied by ChatGPT. On the first successful Zodex invocation by an Agent in that workdir, it checks in this order:

```text
<workdir>/AGENTS.override.md
<workdir>/AGENTS.md
```

If one exists, the result gets one short hint such as:

```text
/Users/me/code/project contains an AGENTS.md.
```

The repo file itself is not injected. The check is performed at most once for that Agent and normalized workdir. If neither file exists, nothing is appended and that check is still considered consumed. A Zodex/tool execution failure does not consume the check, so a later successful invocation can still receive the hint. A command that runs normally and exits non-zero still counts as a successful invocation of the workdir.

This is deliberately narrower than Codex's own project-instruction loading. Codex can walk applicable instructions through a project hierarchy; Zodex only tells ChatGPT about an instruction file at the exact workdir it requested, leaving the Agent to read and interpret it itself.

Disable workdir hints with:

```bash
zodex local config set context.repo-agents false
```

### Turn all automatic context off

```bash
zodex local config set context.enabled false
```

This master switch suppresses global instructions, the global skill catalog, and workdir hints without changing the individual settings underneath it.

The global instructions/skills block is delivered at most once per ChatGPT conversation. Zodex keeps only small hashed delivery markers for this purpose, so restarting Local or pruning old Local history does not make the context repeat and does not require retaining the raw OpenAI session key or old workdir path in the delivery-state tables.

## History retention

Defaults:

```text
history.max-age  = 60d
history.max-size = 500mb
```

Examples:

```bash
zodex local config set history.max-age 14d
zodex local config set history.max-size 2gb
```

Retention removes old complete invocation history. It does not intentionally keep half an invocation just to hit a byte limit.
Zodex prunes old complete history before a new Local runtime accepts commands, then reclaims physical database pages in bounded increments while it runs. Shutdown flushes queued evidence but does not wait for physical database compaction.

## Tunnel ID

`zodex local setup` normally writes the tunnel ID for you. You can inspect it with:

```bash
zodex local config get tunnel.id
```

You can change it while Local is stopped:

```bash
zodex local config set tunnel.id tunnel_<id>
```

When changing credentials or repairing the managed tunnel installation, prefer rerunning:

```bash
zodex local setup
```

because setup validates the tunnel/key combination and the managed tunnel client.

## What is not stored in Local config

The OpenAI runtime API key is not a normal config value. Zodex stores it in macOS Keychain.

The Local observability bearer is also managed separately and automatically. You do not need to put it in ChatGPT or in shell config.

The short-lived MCP credential used between the managed tunnel and the Local MCP listener exists only for an active runtime.

## Where Local state lives

You usually do not need these paths, but they help with backup or troubleshooting:

```text
~/.config/zodex/local.toml
~/.local/state/zodex/local/history/
~/.local/state/zodex/local/logs/
~/.local/state/zodex/local/runtime/
```

XDG environment overrides may move the exact roots.

Treat the history database as private audit data: it can contain commands, tool arguments, output, paths, and secrets that appeared in tool calls.

## Environment changes

Local captures the environment of the process that runs `zodex local start`. If you install a new CLI, change PATH, switch a toolchain, or export a variable in a later shell, restart Local to capture the new environment:

```bash
zodex local stop
zodex local start
```

Aliases and shell functions are not part of the compatibility promise; PATH-visible commands and normal environment/toolchain configuration are.

## Related guides

- [Local setup and ChatGPT connection](/docs/local/setup)
- [Local daily use](/docs/local/daily-use)
- [Local troubleshooting](/docs/local/troubleshooting)
