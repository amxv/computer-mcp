---
title: "Sprite"
description: "Set up a remote Sprite-backed Linux workspace for ChatGPT with isolated GitHub read access and operator-controlled write modes."
order: 1
category: Start
summary: "Install Zodex, create a Sprite, configure GitHub Apps, connect ChatGPT to MCP, and choose how much GitHub write autonomy the Agent gets."
---

Sprite mode gives ChatGPT a remote Linux machine that is separate from your personal computer. It is the Zodex path for persistent remote workspaces, Linux tooling, and an explicit GitHub autonomy model.

If you want ChatGPT to use the files, credentials, and developer tools on your own Apple Silicon Mac instead, use [Zodex Local](/docs/local).

## What Sprite mode gives you

A Sprite deployment has three practical boundaries:

1. **The remote Linux workspace** — ChatGPT can run commands, edit files, test, and commit there.
2. **Always-on GitHub read access** — a narrow reader GitHub App lets the Sprite clone and fetch approved repositories.
3. **A separate GitHub write policy** — PR publishing, one-off push grants, or operator-controlled YOLO mode decide when direct writes are allowed.

That means an Agent can be productive immediately without automatically receiving permanent GitHub write credentials.

## Sprite quickstart

### 1. Install the Zodex operator CLI

On your own machine:

```bash
curl -fsSL https://zodex.ashray.xyz/install.sh | ZODEX_INSTALL_MODE=operator bash
zodex --version
```

The installer chooses operator mode automatically on a normal macOS/Linux user account.

### 2. Install and authenticate the Sprite CLI

```bash
curl -fsSL https://sprites.dev/install.sh | sh
sprite org auth
```

Create and select a Sprite:

```bash
sprite create zodex-dev
sprite use zodex-dev
```

Make its URL reachable by ChatGPT:

```bash
sprite url update --auth public
sprite url
```

### 3. Create the two GitHub Apps

Sprite mode uses separate reader and writer apps so clone/fetch access does not imply direct push access.

#### Reader app

Give it only:

```text
Repository permissions:
  Contents: Read-only
```

Install it only on the repositories the Sprite should be able to read. Generate a private key and keep:

```text
reader app ID
reader private-key PEM
```

#### Writer app

Give it:

```text
Repository permissions:
  Contents: Read & write
  Pull requests: Read & write

Device Flow: enabled
User access token expiration: enabled
```

Install it only on repositories that should be eligible for PR publishing or direct-push grants. Keep:

```text
writer app ID
writer client ID
writer private-key PEM
```

See [Sprite GitHub Apps](/docs/github-apps) for the complete creation checklist.

### 4. Provision the Sprite

Run setup from your operator machine:

```bash
zodex sprite setup \
  --sprite zodex-dev \
  --repo owner/repo \
  --reader-app-id <reader-app-id> \
  --reader-pem /absolute/path/to/reader.pem \
  --publisher-app-id <writer-app-id> \
  --publisher-pem /absolute/path/to/writer.pem \
  --default-base main \
  --url-auth sprite
```

For a non-default Sprite organization:

```bash
zodex sprite setup ... --org <org-name>
```

Setup installs the Linux runtime, configures the reader/writer GitHub Apps, synchronizes Sprite Services, and checks the deployment.

The writer app's **client ID** is used by the agent-side device-flow push command. If setup did not receive/store it, add `publisher_client_id` to the Sprite configuration before using `request-push`.

### 5. Verify the deployment

```bash
zodex sprite status --sprite zodex-dev
zodex sprite health --sprite zodex-dev
zodex sprite logs --sprite zodex-dev --service zodexd --lines 100
```

You want the MCP runtime and publisher service healthy before connecting ChatGPT.

If you use the optional Cloudflare front door, verify the Sprite origin before deploying the proxy:

```bash
zodex proxy inspect --sprite zodex-dev
zodex proxy verify-origin --sprite zodex-dev
```

See [Sprite: connect ChatGPT](/docs/proxy-mcp).

### 6. Add the Sprite MCP server to ChatGPT

Get the public Sprite URL:

```bash
sprite url
```

The MCP URL has this shape:

```text
https://<sprite-host>/mcp?key=<zodex-api-key>
```

In ChatGPT, create a custom MCP app/connector and use that full HTTPS URL. When the key is already in the URL, choose **No authentication** for the ChatGPT connector itself.

The app should expose exactly:

```text
exec_command
write_stdin
apply_patch
```

See [MCP tools](/docs/tools) for how Agents use them.

### 7. Clone the repository from ChatGPT

A first Agent workflow typically starts under `/workspace`:

```bash
cd /workspace
git clone https://github.com/owner/repo.git
cd repo
git status
```

The reader App should make clone/fetch work before any GitHub write permission is opened.

### 8. Choose the GitHub write policy

Sprite mode has three levels you can move between without changing the MCP connection.

#### Review-first: publish a PR

```bash
zodex-agent github publish-pr \
  --repo owner/repo \
  --title "Describe the change" \
  --base main \
  --body "Summary and tests."
```

The Agent commits locally; the publisher service pushes a generated branch and opens the PR without handing writer-app credentials to the shell.

#### Temporary direct push

Agent-requested:

```bash
zodex-agent github request-push --repo owner/repo
# normal git push is now available for the grant window
git push origin main
zodex-agent github revoke-push --repo owner/repo
```

Operator-requested:

```bash
zodex github grant-push --sprite zodex-dev --repo owner/repo
zodex github revoke-push --sprite zodex-dev --repo owner/repo
```

#### Trusted-session YOLO

```bash
zodex github mode yolo --sprite zodex-dev --repo owner/repo --ttl 4h
zodex github mode status --sprite zodex-dev
zodex github mode default --sprite zodex-dev
```

Use [Sprite write modes](/docs/write-modes) to choose the right level.

## Day-to-day Sprite commands

```bash
zodex sprite status --sprite zodex-dev
zodex sprite health --sprite zodex-dev
zodex sprite logs --sprite zodex-dev --service zodexd --lines 100
zodex sprite sync --sprite zodex-dev
zodex sprite upgrade --sprite zodex-dev
```

See [Sprite operations](/docs/sprite-operations) for recovery and upgrades.

## The Sprite permissions model in one sentence

**Read access is persistent and narrow; local workspace changes are unrestricted inside the Sprite; GitHub writes happen only through the PR/grant/YOLO policy you choose.**

Read [Sprite permissions and autonomy](/docs/access-model) before giving a new Agent broader direct-push access.

## Sprite guides

- [Sprite GitHub Apps](/docs/github-apps) — create the reader and writer apps.
- [Sprite permissions and autonomy](/docs/access-model) — understand the security boundary.
- [Sprite write modes](/docs/write-modes) — choose PR-only, temporary push, or YOLO.
- [Sprite PRs and push grants](/docs/push-grants) — agent-side write workflows.
- [Sprite operator write controls](/docs/operator-grants) — approve/revoke from your machine.
- [Sprite: connect ChatGPT](/docs/proxy-mcp) — MCP URL and optional proxy.
- [Sprite configuration](/docs/configuration) — server/runtime config.
- [Sprite operations](/docs/sprite-operations) — health, logs, sync, and upgrades.
- [Sprite command reference](/docs/sprite-command-reference) — operator and guest commands.
- [Sprite troubleshooting](/docs/sprite-troubleshooting) — symptom-first recovery.
