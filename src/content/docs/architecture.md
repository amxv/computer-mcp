---
title: "How Zodex works"
description: "Understand the two first-class Zodex deployment paths and the small tool contract they share without diving into repository internals."
order: 1
category: Architecture
summary: "Sprite is a remote Linux workspace with scoped GitHub autonomy; Local is trusted direct Mac execution. Both expose the same three MCP tools."
---

Zodex has two first-class ways to give ChatGPT a real coding machine:

| | Sprite | Local |
| --- | --- | --- |
| Machine | Remote Sprite-backed Linux | Your Apple Silicon Mac |
| Trust model | Isolated remote workspace + scoped GitHub write policy | Trusted host; commands run as your Mac user |
| Connection | Public Sprite MCP URL, optionally proxied | OpenAI Secure MCP Tunnel |
| GitHub permissions | Reader App + publisher/grants/YOLO | Whatever Git/network credentials your Mac user already has |
| Lifecycle | Remote Sprite services | One explicit Mac runtime, optional TTL |
| Observability | Normal command/service tooling | Public localhost observability API + first-party TUI + durable history |

Choose the machine/trust model first. The ChatGPT-facing coding workflow stays familiar in both cases.

## What both modes share

ChatGPT receives only:

```text
exec_command
write_stdin
apply_patch
```

That is enough to:

- inspect repositories;
- edit files;
- run builds/tests;
- keep long-running processes alive;
- poll or send input to those processes;
- use normal Git commands appropriate to the deployment's permissions.

See [MCP tools](/docs/tools).

Every command and patch has an explicit absolute workdir. This keeps routing clear when one server is used by more than one conversation/workspace.

## Sprite at a glance

```text
ChatGPT
   |
public MCP URL
   |
Sprite Linux workspace
   |
reader GitHub App + selected writer policy
```

The Sprite itself is the remote working environment. ChatGPT can change files and run arbitrary commands inside that machine, while GitHub network write access is separately controlled.

Use:

- [Sprite](/docs/quickstart) to set it up;
- [Sprite permissions and autonomy](/docs/access-model) to understand the boundary;
- [Sprite write modes](/docs/write-modes) to choose PR/grant/YOLO behavior.

## Local at a glance

```text
ChatGPT
   |
OpenAI Secure MCP Tunnel
   |
your Mac
   |
your shell, files, developer tools, and user credentials
```

Local is intentionally a trusted-host path. It is not a per-repo sandbox. One running Local service can serve several independent ChatGPT conversations at once, and each tool call names the absolute workdir it wants to use.

Use:

- [Local](/docs/local) to set it up;
- [Local daily use](/docs/local-operations) for the normal workflow;
- [Local troubleshooting](/docs/local-troubleshooting) for Mac/tunnel problems.

## Local start directory versus command workdir

When you run:

```bash
cd ~/code/project
zodex local start
```

Local publishes that directory to ChatGPT as the **suggested initial explicit workdir**. This makes a fresh chat easy to start without silently changing the execution contract.

The actual `exec_command` or `apply_patch` call still contains its own absolute workdir. An Agent can deliberately move to another repo later.

## Local Agents are observation, not permissions

Zodex can group Local tool activity by ChatGPT conversation and show a short Agent ID such as `k7m2` in `watch` and history.

That grouping does not create a separate sandbox, user account, or filesystem permission boundary. It exists so you can tell which conversation did what.

## Local observability is a public client surface

Local exposes a separate read-only localhost HTTP API for runtime state, Agents, invocations, output, and live SSE. It supports server-side Agent filtering, workdir filtering on invocation queries, bounded output pagination, versioned presentation data, and durable recovery after a stream gap or disconnect.

The built-in `zodex local watch` TUI is the first client of that API, not a privileged special case. You can build another client in any language against the same contract: a web dashboard, Swift/menu-bar app, terminal UI, editor integration, desktop app, or automation surface.

See [Local watch TUI](/docs/local-watch) and [Local observability API](/docs/local-watch-client).

## Advanced protocol notes

Most users can stop reading here. These details matter mainly when implementing an MCP client or debugging compatibility.

The shared server foundation uses **RMCP 3.x** and accepts modern **MCP `2026-07-28` stateless requests**. Normal modern calls **do not depend on transport-session state**. Provider correlation metadata is consumed **outside the model-visible tool arguments**, so the model never receives bookkeeping fields such as an Agent ID.

Local's managed tunnel authenticates to the private loopback MCP listener using a per-runtime `X-Zodex-Local-Token`. The OpenAI runtime key is stored in **macOS Keychain**, while the **observability server uses its own automatically managed localhost bearer**. These are separate credentials with different jobs.

Those implementation details should not affect the normal Local setup: `zodex local setup`, `start`, `status`, `watch`, `history`, and `stop` manage them for you.
