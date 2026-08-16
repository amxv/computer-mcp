---
title: "Local daily use"
description: "Start, inspect, observe, and stop Zodex Local while one or more ChatGPT conversations work on your Mac."
order: 1
category: Operations
summary: "The practical Local workflow for runtime TTLs, status, Agents, watch, durable history, logs, and clean shutdown."
---

Once [Local setup](/docs/local-setup) is complete, most days use only `start`, `status`, `watch`, `history`, and `stop`.

## Start from the project you want ChatGPT to begin with

```bash
cd ~/code/my-project
zodex local start
```

The current directory becomes the start directory ChatGPT is shown as a suggested explicit workdir.

You can also provide it directly:

```bash
zodex local start ~/code/my-project
```

### Give access an expiry

```bash
zodex local start --ttl 30min
zodex local start --ttl 4h
zodex local start --ttl 2d
```

The TTL belongs to the one Local runtime. It is not renewed when another ChatGPT conversation starts or when an existing Agent runs more commands.

## Starting again while Local is already running

A second `zodex local start` does not silently switch repositories or change the TTL. It reports the active runtime and returns successfully.

To change runtime-level start state:

```bash
zodex local stop
cd ~/code/another-project
zodex local start --ttl 2h
```

After changing the start directory, open the Zodex Local app in ChatGPT app settings and choose **Refresh** before opening a fresh conversation. ChatGPT caches MCP server instructions, so restarting Local alone cannot replace the start-directory guidance already cached in the app definition.

## Check status

Human-readable status:

```bash
zodex local status
```

Machine-readable status:

```bash
zodex local status --json
```

The human view tells you whether Local is configured/running, when it expires, the start directory, MCP/tunnel readiness, current Agent count, active process count, and history health.

If status says the runtime is stale, run:

```bash
zodex local stop
zodex local start
```

and use [Local troubleshooting](/docs/local-troubleshooting) if it cannot recover.

## Work with several ChatGPT conversations

You can open multiple ChatGPT conversations against the same Local app. Zodex automatically groups each attributed conversation under a short Agent ID such as `k7m2`.

The conversations can work concurrently in different repositories or worktrees. Their tool calls carry explicit absolute workdirs, so you do not need a daemon per repository.

Local does not restrict an Agent to its first workdir. A new declared workdir is recorded so unexpected workspace drift is visible, but it is not blocked.

## Watch live activity

`zodex local watch` is the first-party client for Local's public read-only observability API. It discovers the running localhost API, authenticates automatically, subscribes to the live SSE stream, and uses the same Agent/invocation/presentation resources available to your own clients.

```bash
zodex local watch
```

Behavior:

- zero Agents: wait for activity;
- one Agent: open it directly;
- multiple Agents: show a picker;
- unattributed activity: show it explicitly rather than inventing an Agent.

Watch one Agent in a dedicated terminal:

```bash
zodex local watch --agent k7m2
```

Watch a combined stream:

```bash
zodex local watch --all
```

Useful keys include:

- arrows or `j` / `k` — move;
- `Enter` — expand/collapse;
- `Tab` / `Shift-Tab` — cycle Agents;
- `a` — Agent picker;
- `r` — raw evidence view;
- `y` — copy;
- `g` / `G` — top/bottom;
- `/` — search;
- `q` — quit.

Closing `watch` never stops the runtime.

For the complete TUI behavior and keyboard map, see [Local watch TUI](/docs/local-watch). To build another interface on the same API, see [Local observability API](/docs/local-watch-client).

## Inspect history

Recent activity:

```bash
zodex local history
zodex local history --last 20
zodex local history --since 2h
```

One Agent:

```bash
zodex local history --agent k7m2
zodex local history --agent k7m2 --since 1h
```

One declared workdir:

```bash
zodex local history --workdir /absolute/repo/path
```

One invocation:

```bash
zodex local history --id <invocation-id>
```

Exact logical tool evidence:

```bash
zodex local history --id <invocation-id> --raw
```

Structured output:

```bash
zodex local history --format json
```

History works while Local is running or stopped.

## Clear history

Local must be stopped first:

```bash
zodex local stop
zodex local history clear
```

For non-interactive cleanup:

```bash
zodex local history clear --yes
```

Clearing history also removes retained Agent mappings, so a conversation seen again later may receive a new short Agent ID.

## Read diagnostic logs

```bash
zodex local logs
zodex local logs --lines 500
```

Use logs for setup/start/tunnel lifecycle problems. Use `history` for what ChatGPT actually asked Zodex to do.

## Stop Local

```bash
zodex local stop
```

Stopping Local:

- rejects new command/patch/stdin work;
- shuts down tunnel ingress;
- terminates active Zodex-owned process groups and ordinary descendants where they can be identified;
- finalizes durable history;
- removes active runtime state.

Persistent configuration, history, and the managed setup survive for the next `start`.

## What happens on logout or reboot?

Local is not an auto-start login service. A later login/reboot requires another explicit:

```bash
zodex local start
```

That is intentional: access is something you turn on, optionally give a TTL, and turn off.

## Next

- [Local watch TUI](/docs/local-watch)
- [Local observability API](/docs/local-watch-client)
- [Local command reference](/docs/local-command-reference)
- [Local configuration](/docs/local-configuration)
- [Local troubleshooting](/docs/local-troubleshooting)
