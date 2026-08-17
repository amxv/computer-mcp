---
title: "Write modes"
description: "Choose PR publishing, a temporary repo push grant, operator-granted push, or scoped YOLO for a Sprite coding session."
order: 4
category: Sprite
summary: "A practical decision guide for how much GitHub write autonomy to give a Sprite Agent."
---

These are the **Sprite GitHub autonomy modes**. They do not change MCP execution authority inside the remote workspace.

| Goal | Mode | Command |
| --- | --- | --- |
| Human review before GitHub change lands | Publish PR | `zodex-agent github publish-pr --repo owner/repo --title "..."` |
| Agent asks for one repo window | Request push | `zodex-agent github request-push --repo owner/repo` |
| Human opens one repo window | Operator grant | `zodex sprite github grant-push --sprite dev --repo owner/repo` |
| Repeated trusted pushes to one repo | Repo YOLO | `zodex sprite github yolo --sprite dev --repo owner/repo --ttl 2h` |
| Trusted session across all eligible repos | Broad YOLO | `zodex sprite github yolo --sprite dev --ttl 2h` |
| Intentional indefinite autonomy | No-TTL YOLO | `zodex sprite github yolo --sprite dev --no-ttl` |

## 1. Publish a PR

Use this first when review is desirable:

```bash
zodex-agent github publish-pr \
  --repo owner/repo \
  --title "Implement feature" \
  --body "Summary and validation"
```

The agent never receives the writer App key/token; `zodex-prd` owns that boundary.

## 2. Agent-requested temporary push

```bash
zodex-agent github request-push --repo owner/repo
```

Complete GitHub Device Flow as instructed. The default TTL is 30 minutes; set `--ttl` or `--no-ttl` deliberately when different behavior is required.

Then use ordinary Git:

```bash
git push origin HEAD
```

Inspect/revoke from the guest:

```bash
zodex-agent github list-grants
zodex-agent github revoke-push --repo owner/repo
```

## 3. Operator-granted repo push

When the human wants to open the window directly:

```bash
zodex sprite github grant-push --sprite dev --repo owner/repo
```

Revoke it independently:

```bash
zodex sprite github revoke-push --sprite dev --repo owner/repo
```

## 4. Repo-scoped YOLO

```bash
zodex sprite github yolo \
  --sprite dev \
  --repo owner/repo \
  --ttl 2h
```

Repeat `--repo` to grant several repositories. Repo-scoped YOLO entries preserve independent expiries rather than replacing unrelated active repo entries.

## 5. Broad YOLO

```bash
zodex sprite github yolo --sprite dev --ttl 2h
```

This removes the policy-side per-repo restriction, but writer App installation/target coverage still independently limits where a push can succeed.

## Inspect and leave YOLO

```bash
zodex sprite github status --sprite dev
zodex sprite github default --sprite dev
```

`default` removes YOLO state. Explicit push grants are separate capabilities and remain until they expire or are revoked.

## Which should I choose?

Prefer the narrowest mode that keeps the workflow moving:

1. PR for review-first work;
2. one repo grant for a small direct-push task;
3. repo-scoped YOLO for repeated trusted work;
4. broad/no-TTL YOLO only when you consciously want that larger autonomy window.
