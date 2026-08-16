---
title: "Configuration"
description: "Read and change Zodex Local's non-secret tunnel and history settings without mixing them with Sprite server configuration."
order: 6
category: Local
summary: "Configure Local retention and tunnel metadata, understand which settings require Local to be stopped, and know which secrets Zodex manages for you."
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

Local must be stopped before configuration changes:

```bash
zodex local stop
zodex local config set history.max-age 30d
zodex local config set history.max-size 1gb
```

If you try to change configuration while Local is active, Zodex tells you to stop it first rather than creating a “current runtime versus next runtime” split.

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
