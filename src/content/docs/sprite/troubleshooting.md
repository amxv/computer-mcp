---
title: "Troubleshooting"
description: "Diagnose the Sprite wake origin, managed services, canonical Worker, MCP capability, GitHub reader, publisher, push grants, and YOLO policy."
order: 11
category: Sprite
summary: "A symptom-first recovery guide for the current wake-on-demand Sprite architecture."
---

Start with the end-to-end check:

```bash
zodex sprite health --sprite dev
```

Then isolate the first failing layer instead of repeatedly retrying the whole workflow.

## 1. Sprite identity / raw origin

```bash
sprite info dev
```

The canonical Worker needs `url auth: public`. Repair with:

```bash
sprite config update --url-auth public dev
sprite info dev
```

The raw URL is only the Worker upstream/wake origin; do not register it directly in ChatGPT.

## 2. Managed service state

```bash
zodex sprite status --sprite dev
zodex sprite logs --sprite dev --service zodex-prd --lines 100
zodex sprite logs --sprite dev --service zodexd --lines 100
```

If definitions drift:

```bash
zodex sprite sync --sprite dev
```

If definitions are correct but a service needs repair:

```bash
zodex sprite restart --sprite dev
```

Neither command changes persistent VM power state.

## 3. Cloudflare Worker

```bash
zodex sprite proxy status --sprite dev
zodex sprite proxy verify --sprite dev
```

Common cases:

### Worker is stale or foreign

```bash
zodex sprite proxy deploy --sprite dev
zodex sprite proxy verify --sprite dev
```

### Several Cloudflare accounts are available

Choose explicitly:

```bash
zodex sprite proxy deploy --sprite dev --cloudflare-account <id-or-name>
```

### Temporary claim expired

Rerun deploy to create a fresh temporary deployment/claim URL. Treat the new claim URL as a secret and claim it within the displayed 60-minute window.

### Temporary Worker was claimed but not permanently registered

```bash
wrangler login --use-keyring
zodex sprite proxy deploy --sprite dev
```

The claim step and normal Wrangler authentication are separate. Zodex never persists the claim URL.

### No Wrangler-capable runner

Install/provide a supported Node/Wrangler environment or make `wrangler`, `bunx`, or `npx` available on `PATH`. Zodex intentionally does not install a general-purpose JS toolchain itself.

## 4. ChatGPT endpoint

```bash
zodex sprite connect --sprite dev
```

If the Worker is current/reachable, this copies the secret capability URL. Use `--show-url` only when you want it printed.

If ChatGPT sees no tools, check:

- the app endpoint came from `zodex sprite connect`;
- Developer Mode/custom apps are enabled for the account/workspace;
- current plan/workspace policy supports the required MCP behavior;
- tools were rescanned after endpoint changes.

Current OpenAI guidance supports full write/modify custom MCP on Business and Enterprise/Edu on ChatGPT web; Pro custom MCP is read/fetch-only. See [OpenAI's current guide](https://help.openai.com/en/articles/12584461-developer-mode-and-mcp-apps-in-chatgpt).

## 5. Reader GitHub failures

Recheck the **reader App**:

- Contents: Read-only;
- installed for the exact repository;
- App ID/installation correspond to its PEM;
- reader PEM ownership remains intact.

Setup/upgrade health checks should catch most of these failures.

## 6. PR publishing fails

```bash
zodex sprite logs --sprite dev --service zodex-prd --lines 100
```

Check:

- worktree is clean/committed;
- requested `owner/repo` matches the checkout origin;
- writer App is installed for that repository;
- writer permissions include Contents, Pull requests, and Workflows read/write;
- writer PEM remains readable only by the publisher boundary;
- bundle is within the default 128 MiB ceiling.

Do not fix this by making the writer PEM readable to `zodex-agent`.

## 7. Direct push fails

Inspect both explicit grants and YOLO policy:

```bash
zodex sprite github list-grants --sprite dev
zodex sprite github status --sprite dev
```

A valid push needs the exact repo to be covered by the active grant/YOLO path **and** by the writer App installation/target configuration.

Open a grant deliberately:

```bash
zodex sprite github grant-push --sprite dev --repo owner/repo
```

Or scoped YOLO:

```bash
zodex sprite github yolo --sprite dev --repo owner/repo --ttl 2h
```

## 8. Setup failed after runtime became healthy

Setup is resumable. A Cloudflare deployment/auth failure does not require you to wipe or rebuild the healthy guest runtime. Fix the reported Worker/auth prerequisite and rerun setup or:

```bash
zodex sprite proxy deploy --sprite dev
```

## Before invasive manual repair

Inspect active work first, then consider a checkpoint:

```bash
sprite checkpoint create -s dev --comment "before manual recovery"
```
