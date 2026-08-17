---
title: "Operator controls"
description: "Open, inspect, and revoke Sprite direct-push access from the operator machine, including repo grants and scoped YOLO."
order: 7
category: Sprite
summary: "The zodex sprite github commands for grants, YOLO policy, status, and independent revocation."
---

These commands run on the **operator machine**, outside the restricted ChatGPT guest shell.

## Grant one repository

```bash
zodex sprite github grant-push \
  --sprite dev \
  --repo owner/repo
```

If needed, supply the writer App Client ID explicitly:

```bash
zodex sprite github grant-push \
  --sprite dev \
  --repo owner/repo \
  --publisher-client-id <writer-client-id>
```

The grant material installed on the Sprite is repo-scoped. The writer PEM/token is not exposed to `zodex-agent`.

## List explicit grants

```bash
zodex sprite github list-grants --sprite dev
```

## Revoke one grant

```bash
zodex sprite github revoke-push --sprite dev --repo owner/repo
```

`--forget-local-auth` also removes operator-side cached Device Flow auth for that repository when deliberately requested.

## Enable YOLO policy

One repository:

```bash
zodex sprite github yolo --sprite dev --repo owner/repo --ttl 2h
```

Several repositories:

```bash
zodex sprite github yolo \
  --sprite dev \
  --repo owner/repo \
  --repo owner/another-repo \
  --ttl 2h
```

All eligible repositories for a trusted window:

```bash
zodex sprite github yolo --sprite dev --ttl 2h
```

No TTL is an explicit opt-out:

```bash
zodex sprite github yolo --sprite dev --no-ttl
```

## Inspect YOLO state

```bash
zodex sprite github status --sprite dev
```

YOLO stores policy metadata, not writer credentials. Direct push still needs writer App installation/target coverage.

## Return to default policy

```bash
zodex sprite github default --sprite dev
```

This removes YOLO state only. Explicit push grants remain independent until expiry/revocation.
