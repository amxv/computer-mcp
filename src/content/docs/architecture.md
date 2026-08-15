---
title: Runtime architecture
description: Understand how ChatGPT, the operator CLI, Sprite and Local Linux targets, server routes, agent identity, GitHub Apps, and publisher daemon fit together.
order: 3
category: Architecture
summary: The component map for the shared Zodex runtime and the distinct Sprite and Local infrastructure around it.
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

The operator machine uses `zodex`. A Sprite guest uses `zodex-agent`, `zodexd`, and `zodex-prd`. Zodex Local installs the same agent/daemon/publisher roles into its persistent Linux machine and adds a restricted tunnel service for Local MCP ingress. The `zodex-client` binary exists for direct HTTP API testing and automation.

## ChatGPT-first tool shape

zodex is designed for ChatGPT coding sessions. The exposed tool surface is intentionally small and familiar:

```text
exec_command  -> run a shell command
write_stdin   -> poll or continue an interactive session
apply_patch   -> apply a targeted patch
```

That shape matches the command/stdin/patch workflow GPT models already handle well. GitHub write operations are not exposed as separate remote tools. ChatGPT uses normal shell Git commands, and zodex decides whether credentials are available for the selected repo and write mode.

## Operator CLI

`zodex` handles setup and operations:

```bash
zodex sprite setup --sprite dev-sprite --repo amxv/zodex --reader-app-id 123456 --reader-pem /secure/zodex/reader.pem --publisher-app-id 987654 --publisher-pem /secure/zodex/push-grant.pem
zodex sprite status --sprite dev-sprite
zodex sprite logs --sprite dev-sprite --service zodexd --lines 100
zodex proxy verify-origin --sprite dev-sprite
zodex github grant-push --sprite dev-sprite --repo amxv/zodex
zodex local status
zodex local start --ttl 2d
zodex github mode yolo --local --repo amxv/zodex --ttl 2d
```

It also contains local service commands such as `install`, `start`, `stop`, `restart`, `status`, `logs`, `set-key`, `rotate-key`, and `tls setup` for direct non-Sprite service control.

## Sprite runtime

`zodexd` is the daemon that serves:

- `/health`, public health check
- `/mcp` and `/mcp/`, MCP transport behind query-key auth
- `/v1/exec-command`, `/v1/write-stdin`, and `/v1/apply-patch`, HTTP JSON endpoints behind Bearer auth

`zodex-prd` is the internal publisher-side service used by the push-grant and publishing support path. It is not exposed as an MCP tool.

## Local runtime

Zodex Local owns one persistent Apple Container Linux machine named `zodex-local`. It does not mount the operator's macOS home directory. Repositories, build outputs, package caches, and installed Linux tools stay on the machine's native persistent filesystem across `local stop` and later `local start`.

The model-facing runtime keeps the same identities and MCP protocol as Sprite, but Local infrastructure is deliberately different:

- `zodex-agent` remains the unprivileged coding identity
- `zodex-publisher` and `zodex-tunnel` remain separate restricted identities
- `zodexd`, `zodex-prd`, the tunnel process, and model-launched descendants share one root-owned restricted Linux network namespace
- that namespace can reach public IPv4 destinations but is denied macOS/private-network and other non-public IPv4 ranges; IPv6 is disabled there
- `zodex local exec` stays outside that namespace as trusted guest-root operator administration
- Secure MCP Tunnel ingress is enabled only by `zodex local start --ttl ...` and revoked by stop/expiry

Local trusts the Apple Container guest kernel and root-owned control plane. Its isolation claim is about the unprivileged coding agent; it is not a hostile-guest-kernel containment boundary.

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

A normal ChatGPT coding session looks like this on either target:

1. ChatGPT connects to the selected target identity: the Sprite MCP route or the dedicated Local Secure MCP Tunnel.
2. `zodexd` authenticates the `key` query parameter.
3. ChatGPT runs shell commands through `exec_command`.
4. Long-running commands return a `session_handle`.
5. ChatGPT polls or writes stdin through `write_stdin`.
6. File edits are applied through shell commands or `apply_patch`.
7. Git clone and fetch use reader-backed access.
8. Work returns to GitHub through PR-only publishing, a push grant, or operator YOLO mode.

The design keeps code execution powerful while making GitHub writes explicit and time-bound. Local machine access and GitHub write authority are separate state machines: starting a Local TTL does not enable direct push, and a Local YOLO grant does not expose the MCP tunnel.

## Why Sprites fit this model

A ChatGPT coding session often happens in bursts: clone, inspect, run checks, patch, wait for feedback, then continue later. Sprites fit that pattern better than an always-on VPS because the workspace can be provisioned for remote work without treating idle time as the default operating mode.

zodex keeps the Sprite-specific operations in the operator CLI so ChatGPT can focus on the coding loop instead of infrastructure setup.

Local follows the same principle without pretending its lifecycle is Sprite-shaped. Apple machine creation, persistent storage, Secure MCP Tunnel lifecycle, resource sizing, and network isolation stay Local-specific while the actual agent/runtime/GitHub behavior is reused.
