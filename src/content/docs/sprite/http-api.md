---
title: "HTTP API"
description: "Use zodex-client or direct HTTP calls against the traditional zodexd command API for Sprite/direct-service automation."
order: 10
category: Sprite
summary: "The advanced /v1 command API behind zodex-client, including Bearer authentication and command/stdin/patch request shapes."
---

Most ChatGPT users should use MCP. This page is for automation or debugging that intentionally calls the traditional `zodexd` JSON API directly.

## Prefer `zodex-client`

The thin client keeps URL/token selection and JSON request details out of scripts:

```bash
zodex-client exec-command --workdir /absolute/path -- command args...
```

Long-running command sessions continue with the client `write-stdin` command, and patches use the client patch command.

Run:

```bash
zodex-client --help
```

for the exact installed-version flags.

## Authentication

The `/v1/*` command API uses Bearer authentication with the Zodex service API key.

Conceptually:

```http
Authorization: Bearer <zodex-api-key>
```

Do not put the key in committed scripts or logs.

## Command execution

The execution route accepts the same logical command fields used by the shared service:

```json
{
  "cmd": "git status --short --branch",
  "workdir": "/absolute/path/to/repo",
  "yield_time_ms": 1000,
  "timeout_ms": 10000
}
```

`workdir` is required and must be an absolute existing directory.

A finished command returns output/status immediately. A command that is still running returns a session handle that can be continued.

## Continue a process

Use the stdin/session route with the returned `session_handle` to:

- poll without sending characters;
- send input;
- kill the process.

That is the same process-session behavior ChatGPT gets from `write_stdin`.

## Apply a patch

Patch requests include:

```json
{
  "patch": "*** Begin Patch\n...\n*** End Patch",
  "workdir": "/absolute/path/to/repo"
}
```

The workdir is required and explicit.

## When to use MCP instead

Use the MCP endpoint when the caller is ChatGPT or another MCP-capable agent. It exposes only the three purpose-built tools and avoids writing a custom client around HTTP request/response details.

See [MCP tools](/docs/reference/tools).
