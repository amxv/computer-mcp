---
title: "Sprite permissions and autonomy"
description: "Understand the Sprite-only boundary between remote workspace access, GitHub reads, PR publishing, temporary push grants, and YOLO mode."
order: 4
category: GitHub Access
summary: "The security model behind the reader app, publisher app, PR path, push grants, TTLs, repository scopes, and revocation."
---

Sprite mode deliberately separates **working on code** from **writing back to GitHub**.

An Agent can edit files, run tests, and make local commits in the remote Linux workspace. GitHub clone/fetch comes from a reader App. GitHub writes are controlled separately by PR publishing, temporary push grants, or operator-controlled YOLO mode.

That separation is the main Sprite security boundary.

## Default state

After setup:

- clone/fetch works for repositories installed on the reader App;
- commands, patches, tests, and local commits work in the Sprite workspace;
- `zodex-agent github publish-pr` can publish reviewed work through the writer App;
- ordinary `git push` is blocked until a direct-push grant or YOLO policy allows it.

A coding session therefore starts useful but not fully autonomous on GitHub.

## Tier 1: persistent read access

The reader GitHub App should have only:

```text
Contents: Read-only
```

Install it on **Only select repositories** unless broader read access is intentional.

This credential exists so the Sprite can use normal commands such as:

```bash
git clone https://github.com/owner/repo.git
git fetch origin
```

It cannot be used for a GitHub contents write.

## Tier 2: publisher-mediated PRs

The writer/publisher App has:

```text
Contents: Read & write
Pull requests: Read & write
```

But the Agent does not need the App's installation token in its shell to publish a PR.

```bash
zodex-agent github publish-pr \
  --repo owner/repo \
  --title "Describe the change" \
  --base main
```

The publisher service receives the committed `HEAD`, pushes a generated branch, and opens the PR. This is the preferred review-first path because the powerful writer token stays inside the publisher boundary.

## Tier 3: direct-push autonomy

When normal `git push` is useful, open it deliberately.

### One repository, temporary grant

```bash
zodex-agent github request-push --repo owner/repo
```

or from the operator machine:

```bash
zodex github grant-push --sprite dev --repo owner/repo
```

The default push-grant TTL is `30m`.

### Trusted-session YOLO

```bash
zodex github mode yolo --sprite dev --ttl 2h
```

YOLO is controlled from the operator side. By default it applies to repositories installed for the writer App; use `--repo` to narrow it:

```bash
zodex github mode yolo --sprite dev --repo owner/repo --ttl 4h
```

Use `--no-ttl` only when indefinite write autonomy is intentional.

## Revocation

Close one direct-push grant:

```bash
zodex-agent github revoke-push --repo owner/repo
```

or:

```bash
zodex github revoke-push --sprite dev --repo owner/repo
```

Return YOLO policy to default:

```bash
zodex github mode default --sprite dev
```

`mode default` removes YOLO state; it does not silently revoke unrelated explicit push grants.

## Repository installation is part of the boundary

A narrow permission on an App installed across an entire organization can still be broader than you intended. Prefer **Only select repositories** for both reader and writer Apps and add repos deliberately.

The writer App installation determines the maximum repository set eligible for PR publishing/direct-push policy. Zodex grants can narrow that set further; they cannot expand beyond the App installation.

## Which level should I use?

Use:

- **reader + PR publishing** for new Agents and important repositories;
- **temporary push grants** when a trusted change needs normal Git push once;
- **repo-scoped YOLO** for trusted iterative work where repeated approvals are just friction;
- **broad/no-TTL YOLO** only in environments where that level of autonomy is a deliberate choice.

See [Sprite write modes](/docs/write-modes) for examples and [Sprite GitHub Apps](/docs/github-apps) for the one-time permissions setup.
