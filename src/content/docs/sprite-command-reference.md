---
title: "Sprite command reference"
description: "A compact reference for Sprite setup, status, health, logs, sync, upgrade, proxy, GitHub grant, and guest-agent commands."
order: 4
category: Reference
summary: "The operator and guest commands used to provision, maintain, connect, and control GitHub write access for Sprite deployments."
---

This page is the compact command map for Sprite mode. For a guided setup, start with [Sprite](/docs/quickstart).

## Provision a Sprite

```text
zodex sprite setup [OPTIONS]
```

Required:

```text
--sprite <SPRITE>
--repo <REPO>
--reader-app-id <READER_APP_ID>
--reader-pem <READER_PEM>
--publisher-app-id <PUBLISHER_APP_ID>
--publisher-pem <PUBLISHER_PEM>
```

Optional:

```text
--org <ORG>
--default-base <BASE>        default: main
--url-auth <MODE>            default: sprite
--remote-config <PATH>       default: /etc/zodex/config.toml
```

Example:

```bash
zodex sprite setup \
  --sprite dev \
  --repo owner/repo \
  --reader-app-id <id> \
  --reader-pem /path/reader.pem \
  --publisher-app-id <id> \
  --publisher-pem /path/writer.pem
```

## Status and health

```bash
zodex sprite status --sprite dev
zodex sprite health --sprite dev
```

Common options:

```text
--sprite <SPRITE>
--org <ORG>
```

`status` also accepts `--remote-config`; `health` also accepts `--url-auth`.

## Logs

```bash
zodex sprite logs --sprite dev --service zodexd --lines 100
zodex sprite logs --sprite dev --service zodex-prd --duration 10m
```

Options:

```text
--sprite <SPRITE>
--service <SERVICE>   required
--org <ORG>
--lines <LINES>
--duration <DURATION>
```

## Sync Sprite Services

```bash
zodex sprite sync --sprite dev
```

Options:

```text
--sprite <SPRITE>
--org <ORG>
--remote-config <PATH>
--force-recreate
--skip-stop-detached
```

Use `--force-recreate` when service definitions/runtime state need a clean rebuild rather than a no-op sync.

## Upgrade the Sprite runtime

```bash
zodex sprite upgrade --sprite dev
zodex sprite upgrade --sprite dev --version v0.2.27
```

Options include:

```text
--sprite <SPRITE>
--org <ORG>
--version <VERSION>       default: latest
--repo <REPO>
--url-auth <MODE>
--remote-config <PATH>
```

## Proxy inspection

```bash
zodex proxy inspect --sprite dev
zodex proxy verify-origin --sprite dev
zodex proxy deploy --sprite dev
```

All three accept `--origin`; `deploy` also accepts `--skip-verify-origin`.

See [Sprite: connect ChatGPT](/docs/proxy-mcp).

## Publish a PR from inside the Sprite

```bash
zodex-agent github publish-pr \
  --repo owner/repo \
  --title "Title" \
  --base main \
  --body "Summary and tests."
```

## Request/revoke direct push from inside the Sprite

```bash
zodex-agent github request-push --repo owner/repo
zodex-agent github request-push --repo owner/repo --ttl 2h
zodex-agent github request-push --repo owner/repo --no-ttl
zodex-agent github list-grants
zodex-agent github revoke-push --repo owner/repo
```

## Request a push grant from the operator CLI

The operator binary also exposes the same device-flow request helper when you are running it in the relevant environment:

```bash
zodex github request-push --repo owner/repo
zodex github request-push --repo owner/repo --ttl 2h
zodex github request-push --repo owner/repo --no-ttl
```

Options include `--publisher-client-id` and `--cache-refresh-token`. The default TTL is `30m`.

## Grant/revoke push from the operator machine

```bash
zodex github grant-push --sprite dev --repo owner/repo
zodex github revoke-push --sprite dev --repo owner/repo
```

`grant-push` accepts `--publisher-client-id`; `revoke-push` accepts `--forget-local-auth`.

## YOLO mode

```bash
zodex github mode yolo --sprite dev
zodex github mode yolo --sprite dev --repo owner/repo --ttl 4h
zodex github mode yolo --sprite dev --no-ttl
zodex github mode status --sprite dev
zodex github mode default --sprite dev
```

## Shared MCP tools

ChatGPT itself sees only `exec_command`, `write_stdin`, and `apply_patch`. See [MCP tools](/docs/tools).
