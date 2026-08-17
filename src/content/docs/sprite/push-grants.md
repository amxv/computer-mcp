---
title: "PRs and push grants"
description: "Publish PRs from a Sprite, request temporary direct-push access, use normal Git after approval, and revoke grants independently of YOLO."
order: 6
category: Sprite
summary: "The Agent-side Sprite write workflow: publish-pr, request-push, list-grants, normal Git push, and revocation."
---

ChatGPT can clone, inspect, edit, test, and commit before direct GitHub push is enabled. Choose a write path only when the work is ready to leave the Sprite.

## Publish a PR

```bash
zodex-agent github publish-pr \
  --repo owner/repo \
  --title "Ship the change" \
  --base main \
  --body "Summary and validation"
```

Add `--draft` when appropriate.

`publish-pr` requires a clean committed checkout and a supported GitHub origin matching the requested repository. The writer App must cover that repository. Writer tokens remain inside `zodex-prd`.

## Request direct push from the Agent

```bash
zodex-agent github request-push --repo owner/repo
```

The command uses the writer App's Device Flow. Setup already receives and validates the writer Client ID, so there should be no manual runtime-config edit first.

Defaults:

- TTL: 30 minutes;
- scope: exact `owner/repo`;
- refresh-token caching: off unless explicitly requested.

After approval, normal Git works:

```bash
git push origin HEAD
```

## Inspect grants

From the guest:

```bash
zodex-agent github list-grants
```

From the operator:

```bash
zodex sprite github list-grants --sprite dev
```

Expired grants are not usable by the credential helper.

## Revoke a grant

Guest:

```bash
zodex-agent github revoke-push --repo owner/repo
```

Operator:

```bash
zodex sprite github revoke-push --sprite dev --repo owner/repo
```

Revocation is per repository.

## When repeated approval becomes noise

Use repo-scoped YOLO instead of turning one grant into an accidental forever credential:

```bash
zodex sprite github yolo \
  --sprite dev \
  --repo owner/repo \
  --ttl 2h
```

Return to default policy:

```bash
zodex sprite github default --sprite dev
```

This does not remove unrelated explicit grants; revoke those separately.

## Bundle size

PR and direct/YOLO bundle submission share the default **128 MiB** ceiling. Oversized bundles are rejected before the publisher performs GitHub work.
