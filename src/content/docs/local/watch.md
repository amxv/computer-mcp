---
title: "Watch"
description: "Use Zodex's first-party terminal observability client to watch ChatGPT Agents, commands, patches, stdin, process state, workdirs, and live output in real time."
order: 4
category: Local
summary: "The first-party client of the Local observability API: Agent picker, filtered live SSE, compact tool timelines, raw drill-down, search, copy, and durable gap recovery."
---

`zodex local watch` is the first-party real-time observability client for Zodex Local. It shows what ChatGPT is asking the Zodex MCP server to do without giving the viewer any execution authority.

The TUI is built entirely on the public [Local observability API](/docs/local/observability-api). It discovers the localhost API, reads the managed observer bearer, loads Agent state, subscribes to live SSE, and recovers missed activity from the same HTTP endpoints available to your own clients.

Opening or closing `watch` never starts Local, stops Local, changes the runtime TTL, or changes what ChatGPT can execute.

## Start watching

Local must already be running:

```bash
zodex local watch
```

The default view adapts to current activity:

- **No current Agents** — waits for the first attributed tool call.
- **One Agent** — opens that Agent directly.
- **Several Agents** — opens an Agent picker.
- **Unattributed activity** — shows it explicitly instead of inventing an Agent identity.

To open one Agent directly:

```bash
zodex local watch --agent k7m2
```

To deliberately combine all Agent activity:

```bash
zodex local watch --all
```

Agent IDs are the same four-character IDs used by `zodex local history` and the observability API.

## What the TUI shows

The normal view is a compact tool timeline rather than a JSON log or terminal emulator.

For commands, it can show:

- Agent ID;
- command;
- declared workdir;
- effective/current cwd when useful;
- live output;
- running/completed/failed state;
- elapsed time and final exit reason.

For edits and process interaction, it can show:

- structured `apply_patch` file changes and diffs when Zodex can prove them;
- recognized file writes when evidence is strong enough;
- explicit stdin writes;
- explicit process kills;
- cross-Agent process continuation attribution;
- new-workdir notices;
- degraded/incomplete evidence instead of pretending capture was complete.

Repeated no-input `write_stdin` polls are folded into a compact aggregate so a long-running command does not fill the screen with dozens of nearly identical rows. The individual calls remain available in durable raw history.

## Keyboard controls

| Key | Action |
| --- | --- |
| `↑` / `k` | Move up |
| `↓` / `j` | Move down |
| `Enter` | Expand or collapse selected detail; choose an Agent in the picker |
| `Tab` | Next Agent |
| `Shift-Tab` | Previous Agent |
| `a` | Open the Agent picker |
| `r` | Toggle exact raw logical evidence for the selected invocation |
| `y` | Copy the selected content |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `/` | Search/filter the visible timeline |
| `q` or `Ctrl-C` | Quit |

Raw mode is an explicit drill-down. The default view uses Zodex's canonical normalized presentation so the same command/file/poll semantics can also be rendered by other API clients.

## Watch separate Agents in separate panes

Because `watch` is only a read-only client, you can run several instances at once:

```bash
zodex local watch --agent k7m2
zodex local watch --agent m4n8
```

This works well with separate Ghostty/Terminal panes. Each Agent-specific client requests a server-side filtered SSE stream, so it does not need to receive and discard unrelated Agent events locally.

You can also use `Tab` / `Shift-Tab` inside one TUI when a single pane is more convenient.

## Live means from now

`watch` does not preload a large page of completed history when it opens. It subscribes to live activity from the point it attaches.

If the first event refers to a command that was already running, the TUI can fetch that invocation's current detail so the command has enough context to render correctly without loading unrelated old work.

Use `zodex local history` when you intentionally want older activity:

```bash
zodex local history --last 20
zodex local history --agent k7m2 --since 1h
```

## Disconnects and gap recovery

The live event stream is designed so a slow viewer cannot backpressure ChatGPT command execution.

If the TUI disconnects, it reconnects. If the server reports an explicit SSE `gap`, the TUI uses durable invocation/history APIs to recover the missing presentation state before continuing live.

The TUI also validates the active runtime ID and API/presentation versions. If Local restarts into a new runtime or the installed client/server versions no longer agree, reopen `watch` with the matching Zodex binary rather than silently mixing incompatible state.

## Build another interface instead

The TUI is only one possible presentation of Local activity. You can build another viewer on exactly the same public API—for example:

- a localhost web dashboard;
- a Swift or macOS menu-bar app;
- an editor panel;
- another terminal interface;
- a desktop status app;
- an automation or monitoring process.

You do not need to reuse the TUI code. Start with [Local observability API](/docs/local/observability-api), which documents discovery, authentication, routes, filters, output pagination, SSE events, and recovery.
