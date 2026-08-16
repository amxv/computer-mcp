---
title: "MCP tools"
description: "Learn the three tools ChatGPT gets from Zodex and the command, process-session, workdir, and patch patterns that work in both Sprite and Local modes."
order: 1
category: Reference
summary: "The shared ChatGPT-facing surface: exec_command, write_stdin, and apply_patch."
---

Zodex exposes exactly three ChatGPT-facing MCP tools in both Sprite and Local modes:

```text
exec_command
write_stdin
apply_patch
```

The infrastructure and permissions around those tools differ by mode, but the coding workflow is the same.

## `exec_command`

Runs a command in an explicit absolute existing directory.

Inputs:

```text
cmd            required string
workdir        required absolute existing directory
yield_time_ms  optional
timeout_ms     optional
```

Typical request conceptually:

```text
cmd: git status --short --branch
workdir: /absolute/path/to/repo
```

If the command finishes during the initial yield window, Zodex returns its final output/status immediately.

If it is still running, Zodex returns a random `session_handle`. The Agent then uses `write_stdin` to continue it.

There is no backend cwd/default-workdir substitution when the field is missing. The model is expected to send the intended workdir every time.

In Sprite mode, that path is inside the remote Linux workspace.

In **Local mode**, commands run as the trusted logged-in Mac user with the captured developer environment. The Local start directory is supplied to ChatGPT as guidance for the first explicit workdir, not as a filesystem boundary.

## `write_stdin`

Continues a process created by `exec_command`.

Inputs:

```text
session_handle  required
chars           optional
yield_time_ms   optional
kill_process    optional
```

### Poll a long command

Send no characters and no kill flag. Zodex waits for more output or exit and returns the current state.

### Send input

For an interactive prompt:

```text
chars: "y\n"
```

### Kill it

```text
kill_process: true
```

A session handle is a process continuation capability. It is not a ChatGPT conversation ID.

Local records which Agent created/called a process for observability, but Agent identity is not an authorization layer for `write_stdin` in the current Local design.

## `apply_patch`

Applies a Codex-style patch in an explicit absolute existing workdir.

Inputs:

```text
patch    required string
workdir  required absolute existing directory
```

Use it for targeted source/document edits instead of constructing fragile shell replacement commands.

The declared workdir is routing context, not confinement: a deliberately absolute path in a supported patch/command can still reach elsewhere if the deployment's OS permissions allow it.

## Workdir rules

For both `exec_command` and `apply_patch`:

- the field is required;
- it must be absolute;
- the directory must exist;
- invalid workdir input fails before the command/patch side effect.

This makes one shared Zodex server safe to route explicitly across different repos/worktrees without guessing a caller's intended cwd.

## What the tools do not expose

There is no ChatGPT-facing MCP tool for:

- starting/stopping Local;
- changing Local config;
- reading Local history;
- managing the OpenAI tunnel;
- granting Sprite GitHub push/YOLO permissions;
- operating Sprite infrastructure.

Those are operator/CLI concerns. ChatGPT gets the coding surface, not the deployment control plane.

## Permissions depend on the mode

### Sprite

Commands run inside the remote Sprite. GitHub clone/fetch and direct push availability are determined by the Sprite's GitHub Apps and active [write mode](/docs/write-modes).

### Local

Commands run with the Mac user's normal host permissions. There is no Zodex repo sandbox around the tool. macOS privacy controls can still deny protected resources.

See [Local](/docs/local) for the trusted-host boundary.
