---
name: sprites
summary: Operate Zodex Sprite workspaces safely: identify the right user and service boundary, inspect current Sprite state, checkpoint before risky changes, and use the mode-first Zodex operator commands.
---

# Sprites

Use this skill when working on or diagnosing Zodex's **Sprite** mode. Zodex Local is a peer product mode; do not describe Sprite as the universal/default architecture when the task is about Local.

## Mental model

A Sprite is a wake-on-demand remote Linux workspace. Ordinary users do not manually start or stop the VM: incoming HTTP/operator work wakes it and it can sleep again when idle.

The supported ChatGPT path is:

```text
ChatGPT → Zodex Cloudflare Worker → public Sprite wake edge → zodexd → workspace
```

The raw Sprite URL is public so the Worker can wake and reach it. That does **not** make Zodex execution unauthenticated: `/mcp` still requires the secret Zodex query capability. Use `zodex sprite connect` for the connector URL rather than constructing it manually.

## Identity matters

Do not confuse these users:

- `sprite` is the default interactive Sprite user used by `sprite console` / ordinary `sprite exec`.
- `zodex-agent` is the restricted account that executes ChatGPT coding work.
- `zodex-publisher` owns the isolated writer key and publisher daemon state.

When reproducing an agent-visible permission or Git problem, run the check as `zodex-agent`, not merely as `sprite` or root.

Example:

```bash
sprite exec -s dev -- sudo -u zodex-agent -H bash -lc 'id; pwd; git status --short --branch'
```

## Inspect before mutating

Start with current truth:

```bash
sprite info dev
zodex sprite status --sprite dev
zodex sprite health --sprite dev
zodex sprite proxy status --sprite dev
```

For service-specific evidence:

```bash
zodex sprite logs --sprite dev --service zodexd --lines 100
zodex sprite logs --sprite dev --service zodex-prd --lines 100
```

If raw URL auth is wrong, use the current Sprite configuration command:

```bash
sprite config update --url-auth public dev
sprite info dev
```

Do not teach the legacy URL-update subcommand in new guidance.

## Prefer Zodex's service control plane

Use the operator commands instead of ad hoc in-guest daemon management:

```bash
zodex sprite restart --sprite dev
zodex sprite sync --sprite dev
zodex sprite upgrade --sprite dev
```

- `restart` restarts the managed Zodex service stack; it is not VM power control.
- `sync` reconciles desired Sprite Service definitions and is primarily repair/advanced operations.
- `upgrade` replaces the selected Sprite runtime and reconciles/restarts it.

Avoid inventing `zodex sprite start` or `stop` workflows.

## Cloudflare Worker

The Worker is the canonical ChatGPT front door, not an optional workaround. The installed `zodex` binary embeds/materializes its Worker project; do not require a repository checkout or manual `wrangler.jsonc` editing.

Useful commands:

```bash
zodex sprite proxy status --sprite dev
zodex sprite proxy verify --sprite dev
zodex sprite proxy deploy --sprite dev
```

If multiple Cloudflare accounts are available, pass the intended account explicitly:

```bash
zodex sprite proxy deploy --sprite dev --cloudflare-account <id-or-name>
```

On a first unauthenticated deploy, Zodex may surface Wrangler's temporary Worker and one-time claim URL. Treat that claim URL as a secret. Claim it within the provider's displayed window; after claim, establish normal Wrangler auth and deploy/register a permanent Worker:

```bash
wrangler login --use-keyring
zodex sprite proxy deploy --sprite dev
```

## GitHub write policy

Operator-side controls:

```bash
zodex sprite github grant-push --sprite dev --repo owner/repo
zodex sprite github revoke-push --sprite dev --repo owner/repo
zodex sprite github list-grants --sprite dev
zodex sprite github yolo --sprite dev --repo owner/repo --ttl 2h
zodex sprite github status --sprite dev
zodex sprite github default --sprite dev
```

Guest/Agent-side controls:

```bash
zodex-agent github request-push --repo owner/repo
zodex-agent github list-grants
zodex-agent github publish-pr --repo owner/repo --title "Title"
zodex-agent github revoke-push --repo owner/repo
```

Never expose the writer PEM or writer installation token to the agent shell.

## Checkpoint before risky remote changes

If a change could damage user work or make recovery expensive, create a checkpoint first:

```bash
sprite checkpoint create -s dev --comment "before zodex repair"
sprite checkpoint list -s dev
```

Use checkpoints as recovery aids, not as permission to skip inspection or tests.

## Debug before retrying

When an operation fails unexpectedly, inspect the exact layer before repeating it:

1. `sprite info` / URL auth
2. `zodex sprite status`
3. `zodex sprite logs`
4. `zodex sprite proxy status`
5. `zodex sprite proxy verify`
6. GitHub grant/YOLO status when the failure is a push

Repeated retries can hide the actual layer that failed. For MCP mutations specifically, never design a proxy retry that can replay an already-dispatched tool call.
