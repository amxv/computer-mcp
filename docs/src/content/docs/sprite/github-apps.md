---
title: "GitHub Apps"
description: "Create the narrow reader and writer GitHub Apps used by Sprite mode for clone/fetch, PR publishing, push grants, and YOLO."
order: 5
category: Sprite
summary: "The one-time Sprite checklist for reader/writer permissions, Device Flow, private keys, App IDs, Client ID, and selected-repository installation."
---

## Why there are two Apps

Sprite mode intentionally separates read access from write authority:

- the **reader App** supports always-on clone/fetch;
- the **writer App** supports PR publishing and approved direct-push paths.

Create and install both Apps yourself so repository scope remains an explicit user-owned security decision. Zodex does not auto-create Apps through a manifest flow in this setup.

## Reader App

Create a GitHub App with the minimum repository permission:

```text
Repository permissions:
  Contents: Read-only
```

Then:

1. install it on **Only select repositories**;
2. select each repository that Sprite mode should be able to clone/fetch;
3. generate/download a private-key PEM;
4. record the **App ID** and absolute PEM path.

No writer permission belongs on this App.

## Writer App

Create a separate GitHub App with:

```text
Repository permissions:
  Contents: Read & write
  Pull requests: Read & write
  Workflows: Read & write

Device Flow:
  Enabled
```

Then:

1. install it on **Only select repositories**;
2. select only repositories that should support PR/direct-write workflows;
3. generate/download a private-key PEM;
4. record the **App ID**;
5. record the **Client ID** from the same App settings page.

The Client ID is used by GitHub Device Flow and is distinct from the App ID. GitHub requires Device Flow to be enabled in the App settings before a CLI/headless flow can request a device code.

## Feed both Apps to setup

```bash
zodex sprite setup \
  --sprite dev \
  --repo owner/repo \
  --reader-app-id <reader-app-id> \
  --reader-pem /absolute/path/to/reader.pem \
  --publisher-app-id <writer-app-id> \
  --publisher-client-id <writer-client-id> \
  --publisher-pem /absolute/path/to/writer.pem
```

Setup validates **before mutating the Sprite** that:

- the reader/writer App IDs match their PEMs;
- the supplied writer Client ID belongs to that writer App;
- writer Device Flow can issue a device code;
- both Apps are installed for the selected repository;
- the reader can obtain repository read access;
- the writer can obtain the write permissions Zodex requires.

If any check fails, fix the App/installation instead of broadening permissions blindly.

## Runtime isolation

After setup:

- reader key: `/etc/zodex/reader/private-key.pem`, readable for the restricted read path;
- writer key: `/etc/zodex/publisher/private-key.pem`, owned by `zodex-publisher` with restrictive permissions;
- `zodex-agent` must **not** be able to read the writer PEM;
- `zodex-prd` mints/consumes writer installation tokens inside the publisher boundary.

Verify after repairs/upgrades with:

```bash
zodex sprite health --sprite dev
zodex sprite status --sprite dev
```

## Which App controls a GitHub operation?

| Operation | Credential boundary |
| --- | --- |
| clone/fetch/`git ls-remote` | Reader App |
| `publish-pr` | Writer App inside `zodex-prd` |
| agent `request-push` | Writer App Device Flow + repo-scoped grant |
| operator `grant-push` | Writer App Device Flow + repo-scoped grant |
| YOLO direct push | YOLO policy **and** writer installation coverage |

See [Permissions and autonomy](/docs/sprite/permissions) and [Write modes](/docs/sprite/write-modes).
