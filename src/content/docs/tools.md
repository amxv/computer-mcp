---
title: MCP tools
description: Document the three ChatGPT-facing tools exposed by zodexd, their inputs, outputs, annotations, and expected usage patterns.
order: 12
category: Reference
summary: "The exact ChatGPT-facing tool surface: exec_command, write_stdin, and apply_patch."
---

## Tool list

Zodex exposes exactly three ChatGPT-facing MCP tools in both Sprite and Local modes:

```text
exec_command
write_stdin
apply_patch
```

The Sprite/default MCP server instructions identify the remote execution surface. Zodex Local extends the runtime instructions with the active Mac runtime's start directory as the **suggested initial explicit workdir**.

In all modes, the important execution contract is unchanged:

```text
exec_command and apply_patch require an absolute existing workdir supplied by the caller
```

There is no backend cwd/default-workdir substitution when the field is missing. In Local, the published start directory is model guidance, not an implicit argument.

This small surface is deliberate. It gives ChatGPT the primitives GPT models are already good at using: run a shell command, keep a long-running session alive, and apply a targeted patch.

No publisher or PR tool is exposed directly over MCP. In **Sprite mode**, GitHub writes are routed through normal shell commands plus `zodex-agent` commands and the selected Sprite write mode. `publish-pr` is exposed through `zodex-agent github publish-pr`; it routes branch publication and PR creation through the Sprite publisher daemon without exposing a write token to MCP tools.

In **Local mode**, commands run as the trusted logged-in Mac user with the captured developer environment. Zodex does not pretend the Sprite reader/publisher/write-grant policy encloses that host: a shell command can use whatever Git/network credentials and tools the user environment itself makes available. That is part of the trusted-host Local boundary.

Local Agent correlation also stays outside the tool schema. ChatGPT/provider request metadata such as `_meta["openai/session"]` is consumed by the Local server adapter and mapped to a short Agent ID for history/watch/API presentation. The model never receives an `agent_id` argument to populate.

## Tool annotations

Each tool is registered with these annotations:

```text
read_only_hint = true
destructive_hint = false
open_world_hint = false
```

The annotations describe the MCP operation surface from the model/client perspective. They do not mean shell commands cannot modify files. A command like `rm` or a patch can still change the workspace. In Sprite mode, GitHub writes remain controlled by the selected write mode; in Local mode, the trusted Mac user's own environment/credentials define ordinary shell network capability.

## exec_command

Description:

```text
Run a shell command in a required absolute existing workdir
```

Input:

```json
{
  "cmd": "cargo test --quiet",
  "workdir": "/workspace/zodex",
  "yield_time_ms": 1000,
  "timeout_ms": 7200000
}
```

Fields:

- `cmd`: shell command string
- `workdir`: required absolute path to an existing directory; there is no daemon-cwd/default-workdir fallback
- `yield_time_ms`: how long to wait before returning partial output
- `timeout_ms`: command timeout, capped by `max_exec_timeout_ms`

## write_stdin

Description:

```text
Write to or poll a running session
```

Poll:

```json
{
  "session_handle": "session-token",
  "yield_time_ms": 1000
}
```

Write input:

```json
{
  "session_handle": "session-token",
  "chars": "continue
",
  "yield_time_ms": 1000
}
```

Kill session:

```json
{
  "session_handle": "session-token",
  "kill_process": true
}
```

`session_handle` is required.

In Local v1, a valid random session handle is the continuation capability. If a different Agent calls `write_stdin` with that handle, the call remains permitted but creator/caller/cross-Agent attribution is recorded. Agent IDs are observability identities, not authorization tokens.

## apply_patch

Description:

```text
Apply a Codex-style patch using a required absolute existing workdir
```

Input:

```json
{
  "workdir": "/workspace/zodex",
  "patch": "*** Begin Patch
*** Update File: docs/setup.md
@@
-old
+new
*** End Patch
"
}
```

`workdir` is required and must be an absolute path to an existing directory. Relative paths in the patch are resolved against that explicit workdir. The workdir is an execution anchor, not a sandbox: commands and patches can still reference other absolute paths.

## Output model

Command-style tools return:

```json
{
  "summary": "still running after 1.0s; use session_handle session-token to poll",
  "output": "...",
  "status": "running",
  "cwd": "/workspace/zodex",
  "session_handle": "session-token"
}
```

or, after exit:

```json
{
  "summary": "exited 0 after 0.3s",
  "output": "...",
  "status": "exited",
  "cwd": "/workspace/zodex",
  "exit_code": 0,
  "termination_reason": "exit"
}
```

`output` is stripped of ANSI color/control escape sequences before it is returned. `summary` is a single scan-friendly line such as `exited 1 after 0.2s`, `still running after 30.1s; use session_handle session-token to poll`, `timed out after 120.0s`, or `killed after 12.6s`. `termination_reason` can be `exit`, `timeout`, or `killed`.
