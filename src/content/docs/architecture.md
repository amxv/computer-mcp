---
title: Runtime architecture
description: Understand how the shared ChatGPT tool service fits into Sprite mode, trusted-host Local mode, observability, GitHub policy, and the operator/runtime binaries.
order: 3
category: Architecture
summary: The component map for the shared three-tool service, remote Sprite deployment, and direct Apple Silicon Mac Local runtime.
---

## Component overview

The Rust package builds five binaries:

```text
zodex         full operator CLI
zodex-agent   restricted guest-side helper for agents
zodex-client  thin HTTP API client/debug CLI
zodexd        MCP and HTTP daemon
zodex-prd     internal push-grant support daemon
```

The operator machine uses `zodex`. A Sprite guest uses `zodex-agent`, `zodexd`, and `zodex-prd`. Zodex Local uses the operator `zodex` binary itself as the hidden foreground runtime host; it does not require a sibling `zodexd` install on the Mac. The `zodex-client` binary exists for the Sprite/direct service HTTP API testing and automation.

## ChatGPT-first tool shape

zodex is designed for ChatGPT coding sessions. The exposed tool surface is intentionally small and familiar:

```text
exec_command  -> run a shell command
write_stdin   -> poll or continue an interactive session
apply_patch   -> apply a targeted patch
```

That shape matches the command/stdin/patch workflow GPT models already handle well. GitHub write operations are not exposed as separate remote tools. In Sprite mode, ChatGPT uses normal shell Git commands while zodex's reader/publisher/grant policy decides which GitHub credentials are available. In Local mode, ordinary shell network capability comes from the trusted logged-in user's captured developer environment instead.

Every command and patch request carries an explicit absolute existing `workdir`. Local additionally publishes its current runtime start directory in MCP server instructions as the **suggested initial explicit workdir**. The execution layer never silently substitutes that start directory when the model omits `workdir`.

The shared server foundation uses RMCP 3.x and supports modern MCP `2026-07-28` stateless requests: ordinary modern tool calls do not depend on transport-session state. Provider request metadata such as Local's `openai/session` correlation remains request metadata outside the model-visible tool arguments. A narrow legacy initialize compatibility path is retained where current clients need it without turning modern traffic back into session state.

## Two execution modes

### Sprite

Sprite mode runs the shared service on remote Linux. `zodexd` owns the public/proxy-facing MCP route plus the optional direct JSON HTTP API, while the Sprite access model separates reader credentials, publisher credentials, and operator-controlled write grants.

### Local

Local mode runs one trusted-host service on the logged-in Apple Silicon Mac. One runtime composes:

- an authenticated loopback MCP listener for the exact three tools;
- a managed OpenAI Secure MCP Tunnel for ChatGPT ingress;
- a separate bearer-authenticated read-only loopback observer;
- the durable history/presentation store;
- one session/process broker shared by all connected Agents;
- one absolute runtime-wide TTL when configured.

Local deliberately keeps three credentials separate. The OpenAI tunnel runtime key is a durable provider credential stored behind macOS Keychain; each running Local instance generates an ephemeral MCP bearer forwarded by the managed tunnel as `X-Zodex-Local-Token`; and the read-only observability server uses its own automatically managed localhost bearer. None of these credentials replaces another or becomes a model-visible tool argument.

The generated per-user launchd job is an explicit runtime host, not a persistent `KeepAlive` daemon. Local does not add an auto-start policy.

Multiple ChatGPT conversations share the one Local server. Provider request `_meta["openai/session"]` is mapped outside the model-visible tool schema to one retained four-character Agent ID. Workdir and timing are never identity heuristics. Missing provider metadata remains unattributed.

The observer API and `zodex local watch` are read-only views derived from durable evidence; they cannot execute tools or extend the runtime TTL. See [Building a Local watch client](/docs/local-watch-client).

## Operator CLI

`zodex` handles setup and operations:

```bash
zodex sprite setup --sprite dev-sprite --repo amxv/zodex --reader-app-id 123456 --reader-pem /secure/zodex/reader.pem --publisher-app-id 987654 --publisher-pem /secure/zodex/push-grant.pem
zodex sprite status --sprite dev-sprite
zodex sprite logs --sprite dev-sprite --service zodexd --lines 100
zodex proxy verify-origin --sprite dev-sprite
zodex github grant-push --sprite dev-sprite --repo amxv/zodex
```

It also contains local service commands such as `install`, `start`, `stop`, `restart`, `status`, `logs`, `set-key`, `rotate-key`, and `tls setup` for direct non-Sprite service control.

## Sprite runtime

`zodexd` is the daemon that serves:

- `/health`, public health check
- `/mcp` and `/mcp/`, MCP transport behind query-key auth
- `/v1/exec-command`, `/v1/write-stdin`, and `/v1/apply-patch`, HTTP JSON endpoints behind Bearer auth

`zodex-prd` is the internal publisher-side service used by the push-grant and publishing support path. It is not exposed as an MCP tool.

## Agent helper

`zodex-agent` is deliberately smaller than the operator CLI. It forwards a restricted command set to the guest runtime helper:

```bash
zodex-agent show-url --host dev-sprite.example.net
zodex-agent github request-push --repo amxv/zodex
zodex-agent github publish-pr --repo amxv/zodex --title "Improve docs"
zodex-agent github list-grants
zodex-agent github revoke-push --repo amxv/zodex
```

The agent helper can request and revoke direct-push grants, publish PRs through the publisher daemon, print connection URLs, and serve as the Git credential helper.

## Service flow

A normal ChatGPT coding session looks like this:

1. ChatGPT connects to the proxy-backed `/mcp?key=...` route.
2. `zodexd` authenticates the `key` query parameter.
3. ChatGPT runs shell commands through `exec_command`.
4. Long-running commands return a `session_handle`.
5. ChatGPT polls or writes stdin through `write_stdin`.
6. File edits are applied through shell commands or `apply_patch`.
7. Git clone and fetch use reader-backed access.
8. Work returns to GitHub through PR-only publishing, a push grant, or operator YOLO mode.

The design keeps code execution powerful while making GitHub writes explicit and time-bound.

## Local service flow

A Local coding session looks like this:

1. `zodex local setup` provisions non-secret config, Keychain-backed runtime credentials, and the verified managed tunnel bundle.
2. `zodex local start` captures the developer environment and start directory, then launches the one foreground Local host through the user launchd domain.
3. Local starts separate loopback MCP and observability listeners.
4. The managed tunnel connects remote ChatGPT ingress only to the authenticated MCP surface.
5. ChatGPT calls the same three tools with explicit absolute workdirs.
6. Provider conversation metadata maps each conversation to a short Agent identity for history/watch/API presentation.
7. `zodex local stop` or the one absolute TTL removes ingress and shuts down all Agent/process work together.

Local command process groups remain the primary ownership boundary. Normal child/background jobs are retained as owned work even if a shell leader exits, then terminated during whole-runtime shutdown. Intentional self-daemonization/launch-service escape containment is outside the product scope.

## Why Sprites fit this model

A ChatGPT coding session often happens in bursts: clone, inspect, run checks, patch, wait for feedback, then continue later. Sprites fit that pattern better than an always-on VPS because the workspace can be provisioned for remote work without treating idle time as the default operating mode.

zodex keeps the Sprite-specific operations in the operator CLI so ChatGPT can focus on the coding loop instead of infrastructure setup.
