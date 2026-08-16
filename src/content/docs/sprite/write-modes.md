---
title: "Write modes"
description: "Choose PR-only publishing, a temporary repo push grant, operator-granted push, or scoped YOLO for a Sprite coding session."
order: 4
category: Sprite
summary: "A practical decision guide for how much GitHub write autonomy to give a Sprite Agent."
---

These write modes are the **Sprite GitHub autonomy model**.

ChatGPT can clone, inspect, edit, test, and commit inside the Sprite before any direct GitHub push is enabled. When it is time to publish work, choose the smallest write mode that matches the task.

## Quick decision table

| Need | Use | Typical command |
| --- | --- | --- |
| Review before merge | PR-only | `zodex-agent github publish-pr ...` |
| One repo needs normal push briefly | Agent-requested grant | `zodex-agent github request-push --repo owner/repo` |
| Human should open the window | Operator grant | `zodex github grant-push --sprite dev --repo owner/repo` |
| Repeated trusted pushes | Repo-scoped YOLO | `zodex github mode yolo --sprite dev --repo owner/repo --ttl 4h` |
| Trusted session across installed repos | YOLO | `zodex github mode yolo --sprite dev` |
| Intentionally indefinite autonomy | No-TTL YOLO | `zodex github mode yolo --sprite dev --no-ttl` |

## PR-only: the recommended starting point

```bash
zodex-agent github publish-pr \
  --repo owner/repo \
  --title "Describe the change" \
  --base main \
  --body "Summary and tests."
```

Use it when:

- you want a reviewable branch/PR;
- the repository is important or protected;
- you are testing a new model/workflow;
- the Agent does not need ordinary `git push`.

The writer App token stays inside the publisher service.

## Agent-requested push

```bash
zodex-agent github request-push --repo owner/repo
```

Then normal Git works for that repo during the grant:

```bash
git push origin main
```

Defaults:

```text
TTL: 30m
scope: requested repository
```

Change the duration:

```bash
zodex-agent github request-push --repo owner/repo --ttl 2h
```

Disable the TTL only intentionally:

```bash
zodex-agent github request-push --repo owner/repo --no-ttl
```

Revoke:

```bash
zodex-agent github revoke-push --repo owner/repo
```

Use `--forget-local-auth` as well when you deliberately want to clear cached device-flow auth state.

## Operator-granted push

From the operator machine:

```bash
zodex github grant-push --sprite dev --repo owner/repo
```

Revoke:

```bash
zodex github revoke-push --sprite dev --repo owner/repo
```

This is useful when the human should control the exact moment the remote Agent gets direct push access.

## YOLO mode

Open a trusted write window:

```bash
zodex github mode yolo --sprite dev
```

Defaults:

```text
TTL:   2h
scope: all repositories installed for the writer App
```

Narrow it:

```bash
zodex github mode yolo --sprite dev --repo owner/repo --ttl 4h
```

Grant several selected repos:

```bash
zodex github mode yolo \
  --sprite dev \
  --repo owner/repo \
  --repo owner/another-repo \
  --ttl 4h
```

Make a new grant indefinite:

```bash
zodex github mode yolo --sprite dev --no-ttl
```

Inspect:

```bash
zodex github mode status --sprite dev
```

Return to default:

```bash
zodex github mode default --sprite dev
```

Repo-scoped YOLO grants merge with other active repo grants and keep their own expiry.

## Recommended progression

For a new Sprite deployment:

1. Start with PR publishing.
2. Use a temporary push grant when direct push is genuinely useful.
3. Move trusted, high-iteration repos to repo-scoped YOLO.
4. Use broad or no-TTL YOLO only when you intentionally accept that write autonomy.

The MCP connection does not change as you move between these modes.

## Related guides

- [Sprite permissions and autonomy](/docs/sprite/permissions)
- [Sprite PRs and push grants](/docs/sprite/push-grants)
- [Sprite operator write controls](/docs/sprite/operator-controls)
