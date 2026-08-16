---
title: "Troubleshooting"
description: "Start with the troubleshooting guide for the Zodex mode you are actually running."
order: 3
category: Reference
summary: "Separate symptom-first recovery paths for trusted-host Local and remote Sprite deployments."
---

Local and Sprite fail in different places, so do not debug them as one deployment.

## Zodex Local

Use [Local troubleshooting](/docs/local-troubleshooting) for:

- Secure MCP Tunnel setup/permissions;
- ChatGPT developer-mode app visibility;
- Keychain/runtime-key failures;
- Local startup or stale state;
- macOS Files & Folders / Full Disk Access errors;
- missing PATH/toolchain commands;
- Agent/watch/history issues;
- TTL and stop behavior.

Start with:

```bash
zodex local status
zodex local logs --lines 500
```

If you are building a custom observer/dashboard rather than troubleshooting the built-in CLI/TUI, use [the Local observer client guide](/docs/local-watch-client).

## Sprite

Use [Sprite troubleshooting](/docs/sprite-troubleshooting) for:

- Sprite URL/MCP reachability;
- `zodexd` / `zodex-prd` services;
- TLS or Cloudflare proxy routing;
- reader App clone/fetch failures;
- PR publishing failures;
- device-flow push grants;
- YOLO scope/expiry;
- old service/config migrations.

Start with:

```bash
zodex sprite status --sprite dev
zodex sprite health --sprite dev
zodex sprite logs --sprite dev --service zodexd --lines 200
```
