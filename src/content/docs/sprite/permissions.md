---
title: "Permissions and autonomy"
description: "Understand the Sprite boundary between remote workspace execution, GitHub reads, PR publishing, temporary repo grants, and scoped YOLO."
order: 3
category: Sprite
summary: "The security model behind the reader App, isolated writer App, PR path, push grants, TTLs, exact repository scopes, and revocation."
---

Sprite mode separates **working on code** from **writing back to GitHub**.

ChatGPT can inspect files, run tests, edit, and commit inside the remote workspace without receiving broad GitHub writer credentials. GitHub write authority crosses a separate boundary only through the workflows below.

## Binary and identity boundary

| Component | Runs where/as | Authority |
| --- | --- | --- |
| `zodex` | operator machine | setup, Worker, service repair, grants, YOLO |
| `zodex-agent` | Sprite as restricted agent user | coding work plus request/publish/revoke guest commands |
| `zodex-prd` | Sprite as `zodex-publisher` | isolated writer App key and publish/direct-push backend |

The operator CLI is not installed in the agent shell. `zodex-agent` must not be able to read the writer PEM.

## Read access

The reader GitHub App is installed only on selected repositories and uses **Contents: Read-only**. It supports clone/fetch without giving the agent write permission.

## Review-first PR publishing

```bash
zodex-agent github publish-pr \
  --repo owner/repo \
  --title "Ship the change"
```

The publisher path preserves important checks: clean worktree, supported checkout origin, configured target/installation coverage, ref validation, and writer-token isolation inside `zodex-prd`.

## Temporary explicit push grant

The agent can request a repo-scoped grant:

```bash
zodex-agent github request-push --repo owner/repo
```

Or the operator can open it directly:

```bash
zodex sprite github grant-push --sprite dev --repo owner/repo
```

A grant for `owner/repo` does not make another repository writable. Expired/revoked grants are unusable by the credential helper.

Revoke explicitly:

```bash
zodex sprite github revoke-push --sprite dev --repo owner/repo
```

## Scoped YOLO

For repeated trusted direct pushes:

```bash
zodex sprite github yolo \
  --sprite dev \
  --repo owner/repo \
  --ttl 2h
```

YOLO is **policy metadata, not a writer token**. Direct push still requires both:

1. a current YOLO scope that covers the exact repository; and
2. writer App installation/target coverage for that repository.

Inspect it:

```bash
zodex sprite github status --sprite dev
```

Return to default policy:

```bash
zodex sprite github default --sprite dev
```

`default` removes YOLO state only. It intentionally does **not** delete unrelated explicit repo grants.

## TTLs and no-TTL choices

Agent `request-push` defaults to a 30-minute grant. Operator YOLO defaults to 2 hours. Use explicit TTLs when practical. `--no-ttl` is an intentional opt-out, not a convenience default.

## Bundle ceiling

Both PR publishing and direct/YOLO bundle submission use the same default **128 MiB** Git bundle ceiling. Size does not weaken repository scope, installation coverage, clean-worktree, grant, YOLO, ref, or token-isolation checks.

## The MCP URL is a different credential

The secret `?key=` endpoint created by `zodex sprite connect` authorizes the Zodex MCP service. It is not a GitHub token, a Sprite management token, or a Cloudflare credential. Keep these boundaries separate when diagnosing access failures.
