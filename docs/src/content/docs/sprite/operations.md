---
title: "Operations"
description: "Check end-to-end health, inspect Sprite Service logs, restart/reconcile services, upgrade the runtime, and maintain the canonical Worker."
order: 9
category: Sprite
summary: "Day-to-day commands for keeping a wake-on-demand Zodex Sprite healthy without manually managing VM power."
---

## You do not start or stop a Sprite

Sprite mode is wake-on-demand. Incoming operator/HTTP work wakes the remote environment and it can sleep again when idle. Zodex deliberately has no `zodex sprite start` or `stop` commands.

`restart` means **restart the managed Zodex service stack**, not change VM power state.

## Status and health

Start with:

```bash
zodex sprite status --sprite dev
zodex sprite health --sprite dev
```

- `status` reports Sprite Service state/definition drift and the selected deployment identity.
- `health` verifies the supported chain rather than merely checking a local PID.

Inspect the raw Sprite configuration when needed:

```bash
sprite info dev
```

## Logs

```bash
zodex sprite logs --sprite dev --service zodexd --lines 100
zodex sprite logs --sprite dev --service zodex-prd --lines 100
```

Add `--duration` when you need a provider-supported time window.

## Worker front door

```bash
zodex sprite proxy status --sprite dev
zodex sprite proxy verify --sprite dev
```

Redeploy a stale/missing permanent Worker:

```bash
zodex sprite proxy deploy --sprite dev
```

If multiple Cloudflare accounts are available, choose one explicitly:

```bash
zodex sprite proxy deploy --sprite dev --cloudflare-account <id-or-name>
```

The installed operator embeds the Worker project. Do not manage normal deployments from a Zodex source checkout.

## Restart the Zodex services

```bash
zodex sprite restart --sprite dev
```

This uses dependency-safe service sequencing for `zodex-prd` and `zodexd`. Use it after a service-level failure when the deployed definitions themselves are still correct.

## Reconcile desired service state

```bash
zodex sprite sync --sprite dev
```

`sync` is an advanced repair/reconciliation command. Use it when service definitions drift, after older/manual installations, or when setup/upgrade explicitly directs you there.

The flags `--force-recreate` and `--skip-stop-detached` are recovery controls; do not make them the everyday path.

## Upgrade a Sprite runtime

```bash
zodex sprite upgrade --sprite dev --version latest
```

This is distinct from root `zodex upgrade`, which upgrades only the operator CLI. Sprite upgrade replaces the guest runtime, reconciles/restarts managed services, validates the running version/health, preserves GitHub grant/YOLO state, and updates a registered permanent Worker when the embedded Worker changed and Cloudflare auth is available.

If it reports that the Worker update still needs operator auth:

```bash
wrangler login --use-keyring
zodex sprite proxy deploy --sprite dev
zodex sprite proxy verify --sprite dev
```

## Checkpoint before risky manual repairs

For invasive remote work outside normal Zodex commands:

```bash
sprite checkpoint create -s dev --comment "before manual repair"
sprite checkpoint list -s dev
```

A checkpoint is a recovery aid, not a substitute for inspecting active work/processes first.

## Connect after maintenance

```bash
zodex sprite connect --sprite dev
```

This verifies the registered permanent Worker/build/origin before deliberately revealing/copying the secret MCP capability URL.
