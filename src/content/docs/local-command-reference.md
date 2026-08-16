---
title: "Local command reference"
description: "A compact reference for every public zodex local command, argument, option, and common example."
order: 5
category: Reference
summary: "The public Local CLI: setup, start, status, watch, history, config, logs, and stop."
---

Use this page when you already understand [Local](/docs/local) and just need the exact command surface.

## `zodex local setup`

One-time or repair provisioning.

```text
zodex local setup [OPTIONS]
```

Options:

```text
--tunnel-id <TUNNEL_ID>
--runtime-key-stdin
--runtime-key-env <ENV>
--runtime-key-fd <FD>
--rotate-observability-bearer
```

Interactive:

```bash
zodex local setup
```

Automation:

```bash
printf '%s\n' "$OPENAI_TUNNEL_RUNTIME_KEY" \
  | zodex local setup \
      --tunnel-id tunnel_<id> \
      --runtime-key-stdin
```

`--runtime-key-stdin`, `--runtime-key-env`, and `--runtime-key-fd` are mutually exclusive.

## `zodex local start`

```text
zodex local start [OPTIONS] [PATH]
```

Arguments/options:

```text
[PATH]       start directory; defaults to current directory
--ttl <TTL>  optional wall-clock runtime lifetime
```

Examples:

```bash
cd ~/code/project && zodex local start
zodex local start ~/code/project
zodex local start ~/code/project --ttl 4h
```

Common TTL forms: `30min`, `4h`, `2d`.

## `zodex local status`

```text
zodex local status [--json]
```

Examples:

```bash
zodex local status
zodex local status --json
```

## `zodex local watch`

```text
zodex local watch [OPTIONS]
```

Options:

```text
--agent <AGENT>  watch/wait for one four-character Agent ID
--all            combine all Agents
```

`--agent` and `--all` cannot be combined.

Examples:

```bash
zodex local watch
zodex local watch --agent k7m2
zodex local watch --all
```

## `zodex local history`

```text
zodex local history [OPTIONS] [COMMAND]
```

Options:

```text
--last <N>
--since <DURATION>
--agent <AGENT>
--workdir <ABSOLUTE_PATH>
--id <INVOCATION_ID>
--format <markdown|json>
--raw
```

Examples:

```bash
zodex local history --last 20
zodex local history --since 2h
zodex local history --agent k7m2 --since 1h
zodex local history --workdir /absolute/repo/path
zodex local history --id <invocation-id> --raw
zodex local history --format json
```

### Clear history

```bash
zodex local history clear
zodex local history clear --yes
```

Local must be stopped before clearing history.

## `zodex local config`

### Read settings

```bash
zodex local config get
zodex local config get history.max-age
zodex local config get history.max-size
zodex local config get tunnel.id
zodex local config get tunnel.client-path
```

Readable keys:

```text
history.max-age
history.max-size
tunnel.id
tunnel.client-path
```

### Change a setting

```bash
zodex local config set history.max-age 60d
zodex local config set history.max-size 500mb
zodex local config set tunnel.id tunnel_<id>
```

Writable keys are `history.max-age`, `history.max-size`, and `tunnel.id`. `tunnel.client-path` is inspectable but is managed by setup rather than `config set`.

Local must be stopped before `config set`.

## `zodex local logs`

```text
zodex local logs [--lines <LINES>]
```

`--lines` defaults to `200`.

Examples:

```bash
zodex local logs
zodex local logs --lines 500
```

## `zodex local stop`

```bash
zodex local stop
```

It is idempotent; running it when Local is already stopped is safe.

## Shared MCP tools

The CLI above manages/observes Local. ChatGPT itself receives only the shared [three MCP tools](/docs/tools).
