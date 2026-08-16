---
title: "Configuration"
description: "Configure the remote Sprite server, TLS, session limits, GitHub Apps, workspace defaults, and publisher behavior in /etc/zodex/config.toml."
order: 8
category: Sprite
summary: "The server-side configuration used by zodexd, zodex-prd, the Sprite guest runtime, and GitHub read/write policy."
---

This page is for **Sprite mode**. Zodex Local has a separate user-scoped configuration; see [Local configuration](/docs/local/configuration).

A normal Sprite deployment is configured by `zodex sprite setup`. You usually edit `/etc/zodex/config.toml` only when changing an existing deployment or inspecting a problem.

## Where the config lives

Default:

```text
/etc/zodex/config.toml
```

Read it through the Sprite when debugging:

```bash
sprite exec -- sudo cat /etc/zodex/config.toml
```

Treat the file as sensitive: it contains server keys and paths to private GitHub App credentials.

## Server identity and network settings

The Sprite service config controls the `zodexd` server, including its bind addresses/ports, TLS material, and API/MCP authentication key.

After changing network/TLS settings, resync services and verify health:

```bash
zodex sprite sync --sprite dev --force-recreate
zodex sprite health --sprite dev
```

For public MCP routing, use [Sprite: connect ChatGPT](/docs/sprite/connect).

## `default_workdir`

Sprite server configuration can contain a `default_workdir` for legacy/direct-service contexts and operator presentation.

It is **not** an MCP execution fallback.

The shared ChatGPT tool contract makes `exec_command` and `apply_patch` require an explicit absolute existing `workdir`. A missing or relative workdir is rejected before command/patch side effects.

This is especially useful when several conversations or callers use the same Zodex service: routing stays explicit instead of depending on wherever the daemon happened to start.

## Session settings

The server config includes the limits that govern command sessions, output buffering, default yield behavior, and timeouts.

The safe operating rule is simple: tune these when you have a concrete workload reason, not merely to hide a slow command. Long-running work should normally yield a `session_handle` and continue through `write_stdin`.

See [MCP tools](/docs/reference/tools) for the model-facing behavior.

## Reader GitHub App

The reader App provides persistent clone/fetch access.

Expected repository permission:

```text
Contents: Read-only
```

Its configured App/installation/private-key values must match an installation that includes every repository the Sprite should be able to clone.

If clone/fetch breaks, diagnose the reader path before changing any push policy:

```bash
git ls-remote https://github.com/owner/repo.git HEAD
```

See [Sprite GitHub Apps](/docs/sprite/github-apps).

## Publisher / writer GitHub App

The writer App powers:

- `zodex-agent github publish-pr`;
- temporary direct-push grants;
- operator-controlled YOLO mode.

Expected repository permissions:

```text
Contents: Read & write
Pull requests: Read & write
```

For agent-requested push, also enable Device Flow and user access token expiration and configure the App's client ID.

The writer App installation is the maximum set of repositories that can receive write access; individual grants/YOLO scopes can narrow it further.

## Publisher defaults

`default_base` controls the normal PR base branch when callers do not supply another base. Most repositories use:

```text
main
```

`zodex sprite setup --default-base main` configures it during provisioning.

## URL authentication mode

Sprite setup exposes `--url-auth`, normally:

```text
sprite
```

This controls how operator commands resolve/reach the Sprite URL; it is separate from the Zodex MCP/API key used at the Zodex service itself.

## Applying configuration changes

For changes that affect generated Sprite Services:

```bash
zodex sprite sync --sprite dev
```

When an old/stale service definition needs replacement:

```bash
zodex sprite sync --sprite dev --force-recreate
```

Then validate:

```bash
zodex sprite status --sprite dev
zodex sprite health --sprite dev
```

## Related guides

- [Sprite](/docs/sprite)
- [Sprite GitHub Apps](/docs/sprite/github-apps)
- [Sprite permissions and autonomy](/docs/sprite/permissions)
- [Sprite operations](/docs/sprite/operations)
