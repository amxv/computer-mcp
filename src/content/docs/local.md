---
title: "Local"
description: "Run ChatGPT directly on your trusted Apple Silicon Mac with your normal shell, files, and developer tools."
order: 2
category: Start
summary: "Install Zodex, provision an OpenAI Secure MCP Tunnel, connect ChatGPT, start Local from a repo, and inspect what each ChatGPT Agent does."
---

Zodex Local lets ChatGPT work directly on a **trusted Apple Silicon Mac**. It uses the same three Zodex tools as Sprite mode, but commands run with the logged-in Mac user's normal shell environment and filesystem permissions.

Local is intentionally **not a sandbox**. There is **no Zodex confinement boundary** around the repository you start from. If your user account can read, write, execute, or authenticate to something, a Local tool call can generally do the same, subject to normal macOS privacy controls.

If you want an isolated remote Linux workspace with scoped GitHub write permissions instead, use [Sprite](/docs/quickstart).

## Local quickstart

### 1. Install the Zodex operator CLI

```bash
curl -fsSL https://zodex.ashray.xyz/install.sh | ZODEX_INSTALL_MODE=operator bash
zodex --help
zodex local --help
```

Local is supported on Apple Silicon macOS.

### 2. Create an OpenAI Secure MCP Tunnel

Open [OpenAI Platform tunnel settings](https://platform.openai.com/settings/organization/tunnels) and sign in with the same OpenAI account/email you use for ChatGPT.

1. Choose the Platform organization you want to use.
2. Create a tunnel in **Organization settings → Tunnels**.
3. Associate the tunnel with the ChatGPT workspace that should use Zodex Local.
4. Copy the tunnel ID. It looks like `tunnel_...`.

Creating or editing the tunnel requires **Tunnels Read + Manage**. That is an administrative setup permission; Zodex does not need to keep a Manage-capable key afterward.

### 3. Create a runtime API key with only tunnel Read + Use

Create a scoped Platform API key for the runtime. Give it only:

- **Tunnels: Read**
- **Tunnels: Use**

Do not give the runtime key **Manage** unless you independently need that permission. Zodex Local uses the key to read the selected tunnel and run the tunnel client; it does not need an OpenAI admin key.

Keep both values handy:

```text
Tunnel ID:   tunnel_...
Runtime key: sk-...
```

### 4. Run Local setup

The easiest setup is interactive:

```bash
zodex local setup
```

Zodex prompts for the tunnel ID and runtime key. It stores the runtime key in macOS Keychain, installs and verifies the managed OpenAI tunnel client, and creates the Local state it needs. Setup does **not** leave your Mac remotely accessible when it exits.

For automation, use one of the non-argv secret inputs:

```bash
printf '%s\n' "$OPENAI_TUNNEL_RUNTIME_KEY" \
  | zodex local setup \
      --tunnel-id tunnel_<id> \
      --runtime-key-stdin
```

or:

```bash
zodex local setup \
  --tunnel-id tunnel_<id> \
  --runtime-key-env OPENAI_TUNNEL_RUNTIME_KEY
```

See [Local setup and ChatGPT connection](/docs/local-setup) for the full setup flow and every setup flag.

### 5. Start Local from the repo you want ChatGPT to use

```bash
cd ~/code/my-project
zodex local start
```

Or choose a path explicitly:

```bash
zodex local start ~/code/my-project
```

Add a wall-clock TTL when you want access to expire automatically:

```bash
zodex local start ~/code/my-project --ttl 4h
```

`start` waits until the Local runtime and OpenAI tunnel are ready, then returns. You can close that terminal; Local keeps running until you stop it, its TTL expires, you log out/reboot, or the runtime fails.

The directory you start from is published to ChatGPT as the **suggested initial explicit workdir**. It is a convenience for the model, not a filesystem boundary. Every `exec_command` and `apply_patch` request still carries an explicit absolute workdir, and an Agent can intentionally use another accessible path later.

### 6. Add the tunnel to ChatGPT

ChatGPT developer-mode access and Platform tunnel permissions are separate.

In ChatGPT on the web:

1. Enable developer mode for your account/workspace if needed.
2. Open **Settings → Apps → Create** (or the developer-mode Plugins page).
3. Choose **Tunnel** as the connection method.
4. Select your tunnel, or paste its `tunnel_...` ID.
5. Scan the tools and create the app.
6. Open a fresh chat, enable/select the Zodex app, and ask ChatGPT to inspect or work on the repo you started Local from.

OpenAI currently documents full custom-MCP write/modify support for Business, Enterprise, and Edu workspaces. Pro developer mode supports read/fetch MCP actions, which is not sufficient for Zodex's command and patch tools. Workspace admins may also need to enable developer mode or approve the app.

For workspace-specific steps and tunnel visibility troubleshooting, see [Local setup and ChatGPT connection](/docs/local-setup).

## What Local gives ChatGPT

The model sees exactly three Zodex tools:

- `exec_command` — run a command in an explicit absolute workdir;
- `write_stdin` — continue, poll, type into, or kill a long-running process;
- `apply_patch` — apply a patch in an explicit absolute workdir.

See [MCP tools](/docs/tools) for the shared tool contract.

## The Local trust model

Local is for a machine you intentionally trust ChatGPT to operate.

Commands run as your logged-in macOS user with the environment captured when you run `zodex local start`. That means Homebrew, language toolchains, user-installed CLIs, credentials, Git configuration, and filesystem access work like they do from your normal shell.

This also means the start directory is **not** a permission boundary. A command can use absolute paths, `cd` elsewhere, read another repo, or use credentials available to your account.

macOS remains in charge of its own privacy permissions. Protected locations such as Desktop, Documents, Downloads, iCloud Drive, or app data may require a normal Files & Folders or Full Disk Access grant. Zodex **does not edit TCC databases** or bypass macOS privacy controls.

If you want remote isolation and a GitHub-specific autonomy boundary, use [Sprite permissions and autonomy](/docs/access-model) instead.

## One runtime, many ChatGPT Agents

One Local runtime can serve multiple ChatGPT conversations at the same time. You do not start another daemon or tunnel for each chat.

ChatGPT supplies `_meta["openai/session"]` automatically with tool calls. Zodex uses it to group calls from a conversation and assigns a short **four-character** Agent ID such as `k7m2`. The model does not need to send an Agent ID in tool arguments.

Each Agent can work in a different repository or Git worktree as long as every command/patch supplies the intended absolute workdir.

## One runtime-wide TTL

**One runtime-wide TTL** controls access for the entire Local service.

There is exactly **one** TTL for the whole Local runtime. Starting another chat, creating another Agent, running commands, or opening `watch` never extends it.

Examples:

```bash
zodex local start --ttl 30min
zodex local start --ttl 4h
zodex local start --ttl 2d
```

The TTL is wall-clock time. Sleep does not pause it.

## See what ChatGPT is doing now

Open the read-only terminal viewer:

```bash
zodex local watch
```

With one Agent, it opens that Agent directly. With several Agents, it shows a picker.

Watch one known Agent:

```bash
zodex local watch --agent k7m2
```

Watch all Agents deliberately:

```bash
zodex local watch --all
```

`watch` is only a viewer. Opening or closing it does not start, stop, or extend Local.

## Inspect durable history

History remains available after `watch` closes and after Local stops:

```bash
zodex local history --last 20
zodex local history --agent k7m2 --since 1h
zodex local history --workdir /absolute/repo/path
zodex local history --id <invocation-id> --raw
zodex local history --format json
```

The default history is compact and readable. `--raw` is for exact logical tool input/result evidence when you need to audit a specific invocation.

By default Local keeps history for 60 days or 500 MB, whichever requires cleanup first. See [Local configuration](/docs/local-configuration).

## Stop access

```bash
zodex local stop
```

Stopping Local closes new MCP work, removes the tunnel connection, and attempts to **kill everything normally spawned by Zodex** across all Agents. Deliberately self-daemonized or separately launchd-registered processes are outside the containment promise.

`stop` is safe to run again if Local is already stopped.

## Your normal Local workflow

A typical day looks like this:

```bash
cd ~/code/my-project
zodex local start --ttl 4h

# Use one or more ChatGPT conversations.
# Optional: observe them in another terminal.
zodex local watch

# When finished:
zodex local stop
```

You only need `zodex local setup` again when you want to replace tunnel credentials/configuration or repair/update the managed tunnel setup.

## Local guides

- [Setup and connect ChatGPT](/docs/local-setup) — Platform tunnel, runtime key, setup flags, and ChatGPT app creation.
- [Daily use](/docs/local-operations) — start, status, watch, history, logs, stop, multiple Agents, and TTLs.
- [Configuration](/docs/local-configuration) — retention and non-secret Local settings.
- [Local command reference](/docs/local-command-reference) — every `zodex local` command and flag.
- [Local troubleshooting](/docs/local-troubleshooting) — tunnel, startup, Keychain, privacy, Agent, and history problems.
- [Build a Local observer client](/docs/local-watch-client) — advanced read-only API/SSE guide for dashboard authors.
