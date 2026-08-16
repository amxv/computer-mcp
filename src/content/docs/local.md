---
title: Zodex Local
description: "Run ChatGPT directly on a trusted Apple Silicon Mac with one explicit Local runtime, one service-wide TTL, Agent-aware history/watch, and no persistent auto-start service."
order: 2
category: Start
summary: Set up and operate the trusted-host Local mode on an Apple Silicon Mac without creating a remote Sprite.
---

Zodex Local gives ChatGPT the same three familiar MCP tools as Sprite mode, but runs them **directly on your logged-in Apple Silicon Mac** instead of inside a remote Linux workspace.

Use Local when the Mac itself is the intended trusted coding machine. Use the [Sprite quickstart](/docs/quickstart) when you want a remote, isolated Linux workspace instead.

## Trust model

Local is deliberately powerful. A connected ChatGPT conversation can ask Zodex to run shell commands and edit files as your logged-in user. There is **no Zodex confinement boundary**. The declared `workdir` is routing evidence and an execution anchor, **not a sandbox**: a command can `cd` elsewhere or access other paths your macOS user can access.

macOS still enforces its own privacy controls. Desktop, Documents, Downloads, iCloud data, app containers, and other protected locations can require a normal user-approved Files & Folders or Full Disk Access grant for the effective runtime identity. `zodex local setup` does not edit TCC databases, bypass macOS privacy, or automatically grant access. The exact responsible app/process shown by macOS depends on the installed launch context, so this guide does not guess a grant target: if macOS denies a protected path, use the normal Privacy & Security UI for the identity macOS reports.

Local is currently supported on **Apple Silicon (`aarch64-apple-darwin`) only**.

## What Local runs

There is one Local runtime for the Mac, shared by every connected ChatGPT conversation using that Local connector. It owns:

- one authenticated loopback MCP server exposing exactly `exec_command`, `write_stdin`, and `apply_patch`;
- one managed OpenAI Secure MCP Tunnel process for remote ChatGPT ingress;
- one separate bearer-authenticated loopback observability server for `watch`, dashboards, and read-only clients;
- one durable SQLite history/evidence store;
- all command sessions and normally spawned child/background processes for the runtime.

The runtime is explicitly started. It is **not installed as a persistent auto-start daemon** and does not use `KeepAlive`. Closing the terminal that ran `zodex local start` does not define its lifetime; `zodex local stop` or the optional runtime TTL does.

## 1. Install the operator CLI

Install the Apple Silicon release:

```bash
curl -fsSL https://zodex.ashray.xyz/install.sh | sh
zodex --version
zodex local --help
```

The macOS operator package contains `zodex`. It does not bundle a Local daemon, tunnel credential, history database, runtime token, or tunnel-client state. The official tunnel-client bundle is provisioned later by `zodex local setup`.

Upgrading the operator binary while Local runtime state is active is refused. Stop Local first:

```bash
zodex local stop
curl -fsSL https://zodex.ashray.xyz/install.sh | sh
```

## 2. Provision Local

You need an existing OpenAI Secure MCP Tunnel ID and its runtime key. Run setup interactively:

```bash
zodex local setup
```

Or supply the tunnel ID and read the secret from stdin:

```bash
printf '%s\n' "$OPENAI_TUNNEL_RUNTIME_KEY" \
  | zodex local setup \
      --tunnel-id tunnel_<id> \
      --runtime-key-stdin
```

Automation can also name an environment variable or an already-open file descriptor:

```bash
zodex local setup \
  --tunnel-id tunnel_<id> \
  --runtime-key-env OPENAI_TUNNEL_RUNTIME_KEY
```

Do not put the runtime key itself on argv. Setup stores the runtime credential behind the macOS Keychain boundary, installs and verifies the managed tunnel-client bundle, validates the configured tunnel, creates/reuses the localhost observability bearer, and persists only non-secret Local configuration.

Setup does not start a long-running Local runtime.

## 3. Start from the workspace you want ChatGPT to see first

The smoothest workflow is:

```bash
cd ~/code/owner/repo
zodex local start
```

Or pass the path explicitly:

```bash
zodex local start ~/code/owner/repo
```

The start directory is published in the runtime's MCP instructions as the **suggested initial explicit workdir**. It is not silently substituted by the backend. Every `exec_command` and `apply_patch` request still contains an absolute existing `workdir` field.

That distinction matters for observability: the declared workdir tells you where the model intentionally routed a call. A command can still access or `cd` to other places after it starts.

### One runtime-wide TTL

Add an absolute wall-clock lifetime when you want the service to stop automatically:

```bash
zodex local start --ttl 30min
zodex local start ~/code/owner/repo --ttl 4h
zodex local start --ttl 2d
```

There is exactly **one** TTL for the whole Local runtime. Creating Agents, running commands, attaching `watch`, opening API clients, or becoming active again does not reset, extend, pause, or create per-Agent TTLs. At expiry, remote ingress and all Agent/process work shut down together through the normal whole-runtime stop path.

A healthy second `zodex local start` inspects/reuses the existing runtime; it does not renew the TTL.

## 4. Connect ChatGPT

The OpenAI Secure MCP Tunnel created/provisioned for Local is the remote connector path. Zodex keeps its local MCP bearer out of the tunnel URL and tunnel profile as a literal query secret; the managed tunnel supplies the supported authenticated route to the loopback MCP server.

Once the runtime is ready, ChatGPT sees exactly three tools:

```text
exec_command
write_stdin
apply_patch
```

Modern MCP calls are stateless at the transport layer. Zodex uses the provider-supplied request metadata `_meta["openai/session"]` to correlate calls from one ChatGPT conversation without adding Agent IDs to model-visible tool arguments.

## One server, many Agents

Each retained provider conversation key maps to one stable four-character lowercase-alphanumeric Agent ID such as `k7m2`. Different ChatGPT conversations can work concurrently through the same runtime, including in different repositories or Git worktrees.

Important boundaries:

- the model does not choose or send the Agent ID;
- workdir/timing proximity is never used to merge conversations;
- missing provider metadata remains explicitly unattributed rather than guessed;
- `openai/session` represents the provider conversation correlation key; it should not be assumed to distinguish every possible subagent inside one conversation;
- Agent mappings are observability/history identities, not separate runtime or TTL owners.

## Watch live activity

Open the read-only TUI:

```bash
zodex local watch
```

With one Agent, it opens that Agent directly. With multiple Agents, the default view lets you pick. You can also open dedicated panes:

```bash
zodex local watch --agent k7m2
zodex local watch --all
```

Useful controls include Agent switching/picking, search, expand/collapse, raw drill-down, copy, and normal terminal navigation. The viewer consumes the read-only localhost observability API and SSE stream; attaching viewers never changes the service TTL or command lifecycle.

Agent workdir summaries are **ordered unique declared routing anchors**. Repeated or canonical-equivalent declarations do not invent extra workdirs. When an Agent deliberately declares a new workdir, Local records a new-workdir signal. That summary does not claim the process was confined to those paths.

## Inspect durable history

Compact recent history:

```bash
zodex local history --last 20
zodex local history --since 30min
```

Show what one Agent did in the last hour:

```bash
zodex local history --agent k7m2 --since 1h
```

Filter by declared workdir:

```bash
zodex local history --workdir /absolute/repo/path
```

Inspect exact logical evidence for one invocation:

```bash
zodex local history --id <invocation-id> --raw
```

Machine-readable output is available with:

```bash
zodex local history --format json
```

The default Markdown/JSON presentation is a normalized, bounded view derived from immutable raw evidence. Full PTY output is stored separately from the bounded output returned to ChatGPT and is available through history/API drill-down. Incomplete/degraded capture is labeled rather than silently presented as complete evidence.

Clear retained history only while Local is stopped:

```bash
zodex local stop
zodex local history clear --yes
```

## Cross-Agent process continuation

`write_stdin` session handles are continuation capabilities in v1. If another Agent obtains a valid handle, Local does **not** reject the continuation merely because the caller Agent differs from the process creator.

Instead, creator Agent, caller Agent, and cross-Agent status are retained and shown in history/presentation. Do not treat the four-character Agent ID as an authorization token.

## Status, config, and logs

Human status:

```bash
zodex local status
```

Stable machine-readable status:

```bash
zodex local status --json
```

Local config is user-scoped and separate from Sprite/server `/etc/zodex/config.toml`:

```text
${XDG_CONFIG_HOME:-~/.config}/zodex/local.toml
```

Default history retention is 60 days / 500 MB. Read or change non-secret settings:

```bash
zodex local config get
zodex local config get history.max-age
zodex local config set history.max-age 30d
zodex local config set history.max-size 1gb
```

Local runtime credentials are not stored in this TOML file.

Bounded lifecycle/tunnel diagnostics:

```bash
zodex local logs
zodex local logs --lines 500
```

## Stop and process-cleanup scope

Stop the whole Local runtime:

```bash
zodex local stop
```

Stop closes new MCP side-effect admission, removes remote tunnel ingress, then terminates Zodex-owned command process groups and ordinary discoverable children/background jobs across all Agents before finalizing history/listeners.

The ownership goal is practical, not adversarial containment: **kill everything normally spawned by Zodex**, not mathematically prevent a process from deliberately escaping through a separate launch service or self-daemonization scheme.

## Local files and durability

With default XDG paths, durable Local state lives under:

```text
~/.config/zodex/local.toml
~/.local/share/zodex/bin/                  managed tunnel bundle
~/.local/state/zodex/local/credentials/   observer bearer
~/.local/state/zodex/local/history/       SQLite evidence
~/.local/state/zodex/local/logs/          diagnostics
```

Disposable runtime state is isolated under:

```text
~/.local/state/zodex/local/runtime/
```

That runtime directory contains discovery/state, ephemeral MCP/tunnel artifacts, process ownership state, and the generated launchd job. Stopping/stale cleanup can remove it without deleting durable history, config, logs, or the observer credential.

## Troubleshooting

### `start` says Local is not configured

Run:

```bash
zodex local setup
zodex local status --json
```

### A protected macOS path is denied

This is a macOS privacy boundary, not a workdir-sandbox rule. Use the normal System Settings → Privacy & Security controls for the effective Zodex runtime identity. Do not disable or rewrite TCC. Ordinary unprotected workspaces should not need blanket Full Disk Access.

### An upgrade says Local is running

Stop the runtime before replacing the operator executable:

```bash
zodex local stop
```

### ChatGPT uses the wrong initial repository

Check the active runtime's start directory:

```bash
zodex local status --json
```

Then stop and restart from the intended workspace. The runtime publishes that directory as guidance, but the actual tool call must still contain it explicitly as `workdir`.

### Multiple conversations are hard to distinguish

List status/history, then open dedicated viewers:

```bash
zodex local history --since 1h
zodex local watch --agent <id>
```

For custom dashboards and API clients, see [Building a Local watch client](/docs/local-watch-client).
