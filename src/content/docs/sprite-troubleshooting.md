---
title: "Sprite troubleshooting"
description: "Diagnose Sprite runtime, MCP URL, service, TLS, proxy, GitHub reader, publisher, push-grant, and YOLO problems."
order: 7
category: Reference
summary: "A symptom-first recovery guide for remote Sprite deployments."
---

Start with:

```bash
zodex sprite status --sprite dev
zodex sprite health --sprite dev
zodex sprite logs --sprite dev --service zodexd --lines 200
zodex sprite logs --sprite dev --service zodex-prd --lines 200
```

## The Sprite cannot be reached

Check the public Sprite URL:

```bash
sprite use dev
sprite url
```

Then:

```bash
zodex sprite health --sprite dev
zodex proxy verify-origin --sprite dev
```

If the raw Sprite origin is healthy but the ChatGPT URL is not, inspect the optional proxy separately. See [Sprite: connect ChatGPT](/docs/proxy-mcp).

## `/mcp` rejects ChatGPT

Confirm the connector URL includes the current Zodex key:

```text
https://<host>/mcp?key=<key>
```

If you changed/recreated Sprite config, refresh the ChatGPT connector/app with the current URL.

## `zodexd` is not running

```bash
zodex sprite logs --sprite dev --service zodexd --lines 300
zodex sprite sync --sprite dev --force-recreate
```

Common causes include stale service definitions, port conflicts from older installs, invalid TLS paths, or malformed `/etc/zodex/config.toml`.

## `zodex-prd` is not running

```bash
zodex sprite logs --sprite dev --service zodex-prd --lines 300
```

Check:

- writer App ID/install information;
- writer App private key path/permissions;
- publisher state-directory ownership;
- repository installation scope.

## Clone/fetch fails

The reader App is the first thing to check.

Confirm:

- `Contents: Read-only` is enabled;
- the App is installed on the repository;
- the configured reader App ID/installation is current.

From the Sprite:

```bash
git ls-remote https://github.com/owner/repo.git HEAD
```

If that fails, direct-push grants are irrelevant—the read path must work first.

## `publish-pr` fails

Check the writer App has:

```text
Contents: Read & write
Pull requests: Read & write
```

and is installed on the target repository.

Then inspect publisher logs:

```bash
zodex sprite logs --sprite dev --service zodex-prd --lines 300
```

## `request-push` cannot start device flow

The writer App needs:

- Device Flow enabled;
- user access token expiration enabled;
- its client ID configured/available.

Then retry:

```bash
zodex-agent github request-push --repo owner/repo
```

See [Sprite PRs and push grants](/docs/push-grants).

## `git push` is still denied after approval

List the active grant:

```bash
zodex-agent github list-grants
```

Check:

- the grant repository exactly matches the checkout's GitHub remote;
- the grant has not expired;
- the writer App is installed on that repository;
- local Git is using Zodex's direct-push credential plumbing.

If the local credential state itself is wrong, revoke and forget it before requesting again:

```bash
zodex-agent github revoke-push --repo owner/repo --forget-local-auth
zodex-agent github request-push --repo owner/repo
```

## YOLO is active but a repo cannot push

Inspect scope:

```bash
zodex github mode status --sprite dev
```

A repo-scoped YOLO grant does not authorize a different repo, and Zodex cannot grant access to a repository outside the writer App installation.

## An old deployment conflicts with Zodex

Older Sprite installs may still have legacy services or config paths.

Use:

```bash
zodex sprite sync --sprite dev --force-recreate
```

and inspect active Sprite Services. Remove/disable obsolete services only after confirming they belong to the old deployment.

## Repair versus upgrade

Resync current version/configuration:

```bash
zodex sprite sync --sprite dev --force-recreate
```

Upgrade binaries first, then sync:

```bash
zodex sprite upgrade --sprite dev
zodex sprite status --sprite dev
```

## Still stuck?

Collect non-secret output from:

```bash
zodex sprite status --sprite dev
zodex sprite health --sprite dev
zodex proxy inspect --sprite dev
zodex sprite logs --sprite dev --service zodexd --lines 300
zodex sprite logs --sprite dev --service zodex-prd --lines 300
```

Do not paste private PEMs, API keys, access tokens, or full secret-bearing config into an issue/chat.
