---
title: "Configuration"
description: "Understand the Sprite runtime, GitHub policy, service port, MCP capability key, and publisher settings in /etc/zodex/config.toml."
order: 8
category: Sprite
summary: "The server-side configuration used by zodexd, zodex-prd, the restricted guest runtime, and GitHub read/write policy."
---

This page is for **Sprite mode**. Local has separate user-scoped settings; see [Local configuration](/docs/local/configuration).

## Prefer setup over hand-editing

For a new Sprite, use [the Sprite setup flow](/docs/sprite):

```bash
zodex sprite setup --help
```

Setup writes `/etc/zodex/config.toml`, installs both GitHub App keys with the intended ownership, reconciles Sprite Services, and registers the operator-side Worker metadata. Hand-edit runtime config only when you understand which boundary owns the field.

## Network and MCP fields

The modern Sprite service is **plain HTTP behind the Sprite HTTPS edge**:

```toml
bind_host = "0.0.0.0"
service_port = 8080
api_key = "<secret-capability-key>"
```

There is no in-guest public TLS configuration in the current runtime. The Sprite edge terminates public HTTPS; the Cloudflare Worker uses that public edge as its wake/origin path.

`api_key` is the secret used in the ChatGPT capability endpoint:

```text
https://<worker>.workers.dev/mcp?key=<secret>
```

Treat the full URL as a secret. Routine Zodex status/log/health commands redact it; use `zodex sprite connect` when you intentionally need the endpoint.

## Session and command limits

Representative runtime fields include:

```toml
max_sessions = 64
default_exec_timeout_ms = 7200000
max_exec_timeout_ms = 7200000
default_exec_yield_time_ms = 10000
default_write_yield_time_ms = 10000
max_output_chars = 200000
```

These affect command/session behavior, not Sprite VM power state.

## Reader App

```toml
reader_app_id = 123456
reader_installation_id = 234567890
reader_private_key_path = "/etc/zodex/reader/private-key.pem"
```

The reader App should have **Contents: Read-only** on the selected repositories.

## Publisher boundary

```toml
publisher_socket_path = "/var/lib/zodex/publisher/run/zodex-prd.sock"
publisher_private_key_path = "/etc/zodex/publisher/private-key.pem"
publisher_app_id = 345678
publisher_client_id = "Iv1.example"
publisher_user = "zodex-publisher"
service_group = "zodex"
publisher_branch_prefix = "agent"
publisher_max_bundle_bytes = 134217728
```

The default bundle ceiling is **128 MiB** for PR publishing and direct/YOLO bundle submission. The writer key remains private to `zodex-publisher`; do not weaken its ownership/permissions to debug an agent problem.

Publisher targets/installations scope where writer operations are allowed:

```toml
[[publisher_targets]]
id = "owner/repo"
repo = "owner/repo"
default_base = "main"
installation_id = 456789012

[[publisher_installations]]
account = "owner"
installation_id = 456789012
default_base = "main"
```

A grant or YOLO policy does not bypass this installation/target coverage.

## Guest identity

Typical fields:

```toml
agent_user = "zodex-agent"
agent_home = "/home/zodex-agent"
default_workdir = "/workspace"
```

The model-facing MCP contract still requires explicit absolute `workdir` values; `default_workdir` is guest/runtime context, not a model-visible ambient fallback.

## Raw Sprite URL auth

The canonical Worker requires the raw Sprite origin to be public. Inspect/repair it with current Sprite CLI syntax:

```bash
sprite info dev
sprite config update --url-auth public dev
```

`zodex sprite setup` and `upgrade` reconcile this automatically. Do not switch the origin back to private Sprite auth unless you are deliberately abandoning the supported Worker architecture.

## After a deliberate config change

Use service reconciliation and health checks:

```bash
zodex sprite sync --sprite dev
zodex sprite restart --sprite dev
zodex sprite health --sprite dev
```

If the Worker/runtime relationship changed, also run:

```bash
zodex sprite proxy status --sprite dev
zodex sprite proxy verify --sprite dev
```
