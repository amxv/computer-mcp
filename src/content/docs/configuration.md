---
title: Configuration
description: Configure Sprite/server bind addresses, TLS, session limits and GitHub access, plus the separate non-secret user-scoped Zodex Local settings.
order: 4
category: Architecture
summary: The Sprite/server `/etc/zodex/config.toml` boundary and the separate `~/.config/zodex/local.toml` Local configuration.
---

## Sprite/server config path

All CLIs accept a config path. The default is:

```bash
/etc/zodex/config.toml
```

Use `--config` when operating against another file:

```bash
zodex --config /etc/zodex/config.toml status
zodex-agent --config /etc/zodex/config.toml github list-grants
```

If the file is missing, zodex loads its built-in defaults.

This `/etc/zodex/config.toml` file belongs to the Sprite/server deployment model. **Zodex Local does not put its user runtime credential or observer bearer here.** Its separate user-scoped config is described below.

## Server and API settings

Important runtime defaults:

```toml
bind_host = "0.0.0.0"
bind_port = 443
http_bind_port = 8080
api_key = "zodex-runtime-key"
tls_mode = "auto"
tls_cert_path = "/var/lib/zodex/tls/cert.pem"
tls_key_path = "/var/lib/zodex/tls/key.pem"
```

`zodexd` always expects TLS cert and key files for the TLS listener. Run one of these before starting the daemon directly:

```bash
zodex tls setup
zodex start
```

The optional HTTP listener is controlled by `http_bind_port`. The same app routes are served, but the `/v1/*` endpoints still require Bearer auth.

## Tool execution limits

Execution-related defaults:

```toml
max_sessions = 64
default_exec_timeout_ms = 7200000
max_exec_timeout_ms = 7200000
default_exec_yield_time_ms = 10000
default_write_yield_time_ms = 10000
max_output_chars = 200000
default_workdir = "/workspace"
```

`default_workdir` remains a Sprite/server process/setup default used by operator/runtime lifecycle code. It is **not** an MCP execution fallback. `exec_command` and `apply_patch` require an explicit absolute existing `workdir` in every request. Long-running commands can keep a session open and return a `session_handle` for later polling or stdin writes.

Local follows the same explicit-workdir rule. Its runtime start directory is published to the model as suggested routing guidance only; the backend never substitutes it for a missing field.

## Guest users and paths

Default runtime users and paths:

```toml
agent_user = "zodex-agent"
agent_home = "/home/zodex-agent"
publisher_user = "zodex-publisher"
service_group = "zodex"
publisher_socket_path = "/var/lib/zodex/publisher/run/zodex-prd.sock"
```

The agent should not run as root. The publisher path must be writable by the configured publisher user.

## Reader GitHub App

Reader app fields:

```toml
reader_app_id = 123456
reader_installation_id = 11111111
reader_private_key_path = "/etc/zodex/reader/private-key.pem"
```

The reader app should have only `Contents: Read-only`, and be installed on repositories the runtime may read. Clone/fetch tokens request only `Contents: read`. `publish-pr` is handled by the publisher daemon, not the reader app.

## Push-grant GitHub App

Publisher and grant fields:

```toml
publisher_app_id = 987654
publisher_client_id = "Iv1.real-device-flow-client-id"
publisher_private_key_path = "/etc/zodex/publisher/private-key.pem"
publisher_branch_prefix = "agent"
publisher_max_bundle_bytes = 33554432
publisher_max_title_chars = 240
publisher_max_body_chars = 16000
```

The publisher / push-grant app should have `Contents: Read & write`, `Pull requests: Read & write`, Device Flow enabled, and user access token expiration enabled. Agent-side `publish-pr` sends a local HEAD bundle to the publisher daemon, which uses the publisher app to push a generated branch and open the PR while keeping credentials inside the daemon.

## Publish targets

Publish targets identify repositories that publisher-side flows can operate on:

```toml
[[publisher_targets]]
id = "zodex"
repo = "amxv/zodex"
default_base = "main"
installation_id = 22222222

[[publisher_installations]]
account = "amxv"
default_base = "main"
installation_id = 22222222
```

`publisher_targets` is the explicit allowlist used by `publish-pr`. `publisher_installations` records account-level installations so operator-only GitHub modes can represent an all-installed-repos scope while still staying inside the GitHub App installation boundary.

The day-to-day `request-push` flow uses the repo argument and active grant state. Publish targets are still useful for internal publisher flows and explicit repo allowlists.

## Zodex Local config

Local uses a separate non-secret TOML file:

```text
${XDG_CONFIG_HOME:-~/.config}/zodex/local.toml
```

Inspect it through the CLI rather than depending on its serialized layout:

```bash
zodex local config get
zodex local config get history.max-age
zodex local config get history.max-size
zodex local config get tunnel.id
zodex local config get tunnel.client-path
```

Writable keys are:

```text
history.max-age
history.max-size
tunnel.id
```

Defaults:

```text
history.max-age  = 60d
history.max-size = 500mb
```

Change settings only while Local is stopped:

```bash
zodex local config set history.max-age 30d
zodex local config set history.max-size 1gb
```

The OpenAI tunnel runtime key is stored behind the macOS Keychain boundary. The observer bearer is a user-only credential under Local state. Neither is serialized into `local.toml`.

With default XDG roots, Local's durable paths are:

```text
~/.local/share/zodex/bin/                  managed tunnel bundle
~/.local/state/zodex/local/credentials/   observer bearer
~/.local/state/zodex/local/history/       history.sqlite3
~/.local/state/zodex/local/logs/          local-runtime.log
```

Ephemeral runtime/discovery/tunnel/process state lives separately under:

```text
~/.local/state/zodex/local/runtime/
```

That separation is deliberate: runtime cleanup must not delete Local config, credentials, durable history, or logs.
