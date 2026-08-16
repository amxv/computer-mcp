---
title: "Command reference"
description: "Choose the command reference for Zodex Local or a remote Sprite deployment."
order: 2
category: Reference
summary: "Two distinct CLI surfaces share the zodex operator binary; use the mode-specific reference instead of mixing their lifecycle and permission commands."
---

Zodex has two first-class deployment paths with different operator workflows.

## Local commands

Use [Local command reference](/docs/local-command-reference) for:

```text
zodex local setup
zodex local start
zodex local status
zodex local watch
zodex local history
zodex local config
zodex local logs
zodex local stop
```

These commands operate the trusted direct-Mac runtime.

## Sprite commands

Use [Sprite command reference](/docs/sprite-command-reference) for:

```text
zodex sprite setup
zodex sprite status
zodex sprite health
zodex sprite logs
zodex sprite sync
zodex sprite upgrade
zodex proxy ...
zodex github ...
zodex-agent github ...
```

These commands provision and control a remote Sprite plus its GitHub access policy.

## Shared ChatGPT tools

Both modes expose the same [three MCP tools](/docs/tools):

```text
exec_command
write_stdin
apply_patch
```
