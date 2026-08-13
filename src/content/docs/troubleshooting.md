---
title: Troubleshooting
description: Diagnose ChatGPT connection issues, Local lifecycle/isolation drift, setup failures, push-grant problems, YOLO mismatches, Sprite service failures, TLS errors, and proxy routing issues.
order: 14
category: Reference
summary: Practical failure modes and the commands that usually identify the cause.
---

## ChatGPT cannot connect to a Sprite

Check the public route first:

```bash
zodex proxy verify-origin --sprite dev-sprite
curl https://dev-zodex.example.net/health
```

Then inspect the daemon logs:

```bash
zodex sprite logs --sprite dev-sprite --service zodexd --lines 100
```

Common causes:

- `/mcp` URL is missing the `key` query parameter
- `api_key` in config does not match the URL key
- proxy origin is pointed at the wrong Sprite URL
- TLS files are missing on the Sprite
- `zodexd` is not running or cannot bind its port

## ChatGPT cannot connect to Zodex Local

Start with the operator-side truth:

```bash
zodex local status
```

Do not infer active access from a saved lease alone. Local status requires the machine, daemon, Secure MCP Tunnel, and isolation checks to agree before it reports MCP access as active.

Common Local causes:

- no finite access window is open; run `zodex local start --ttl 2d`
- the previous TTL expired and the host supervisor revoked/stopped the machine
- the machine was stopped outside Zodex
- the tunnel service is inactive or not ready
- the saved Local network-policy identity no longer matches the installed Zodex build
- `zodexd`/`zodex-prd` are unhealthy or not in the required restricted network namespace

For fail-closed cleanup followed by a fresh window:

```bash
zodex local stop
zodex local start --ttl 2d
```

If status reports setup/network drift, re-run the original `zodex local setup ...` command before opening access again.

## `/health` works but `/mcp` is unauthorized

`/health` is public. `/mcp` requires query-key auth:

```text
https://dev-zodex.example.net/mcp?key=secret-runtime-key
```

Rotate or set the key when needed:

```bash
zodex set-key secret-runtime-key
zodex rotate-key
zodex show-url --host dev-zodex.example.net
```

## HTTP API returns unauthorized

The `/v1/*` JSON routes use Bearer auth, not the MCP query parameter:

```bash
curl   -H 'Authorization: Bearer secret-runtime-key'   -H 'Content-Type: application/json'   -d '{"cmd":"pwd"}'   https://dev-zodex.example.net/v1/exec-command
```

Use `zodex-client` when debugging request shapes.

## Git clone fails

Check reader app setup:

- reader app has only `Contents: Read-only`
- reader app is installed on the repository
- `reader_app_id` and `reader_installation_id` are correct
- `reader_private_key_path` points to the installed PEM
- the agent is using the zodex credential helper path

Then test clone from the selected Zodex workspace:

```bash
cd /workspace
git clone https://github.com/amxv/zodex.git
```

## Git push fails

List grants first:

```bash
zodex-agent github list-grants
```

Then check:

- grant repo matches the push target
- grant has not expired
- branch protection allows the intended push
- the writer app is installed on the repository
- the writer app has `Contents: Read & write`
- the operator approved the GitHub device-flow code

Refresh the grant when needed:

```bash
zodex-agent github request-push --repo amxv/zodex
```

## PR creation fails

`publish-pr` does not need a push grant. It creates a generated branch from the current committed `HEAD` through the publisher daemon:

```bash
git status
zodex-agent github publish-pr --repo amxv/zodex --title "Improve docs" --base main
```

Also verify the writer app has `Contents: Read & write` and `Pull requests: Read & write`, the publisher daemon is running, and the repo is listed in `publisher_targets`.

## YOLO is enabled but direct push fails

Check YOLO mode state from the operator machine:

```bash
zodex github mode status --sprite dev-sprite
```

For Local:

```bash
zodex github mode status --local
```

Then check:

- the mode has not expired
- the pushed repo is inside the YOLO scope
- the writer app is installed on that repo
- branch protection allows the intended direct push
- the agent Git helper status printed by `mode status` is healthy
- explicit push grants are not being confused with YOLO state

If the session should no longer be trusted, return to default mode:

```bash
zodex github mode default --sprite dev-sprite
```

Use `--local` for the Local GitHub policy. This does not stop Local MCP access; use `zodex local stop` separately.

## Runtime service cannot start

Inspect status and logs:

```bash
zodex sprite status --sprite dev-sprite
zodex sprite logs --sprite dev-sprite --service zodexd --lines 200
zodex sprite logs --sprite dev-sprite --service zodex-prd --lines 200
```

Check:

- TLS cert and key exist at configured paths
- configured ports are free
- `/var/lib/zodex/publisher` is writable by `zodex-publisher`
- legacy `computer-mcpd` or `computer-mcp-prd` services are not still binding ports
- `/etc/zodex/config.toml` contains the expected repo, app IDs, and paths

## Setup from macOS produces unusable guest binaries

`zodex sprite setup` uploads operator-built runtime binaries. If setup is run from a non-Linux machine, confirm the binaries are compatible with the Sprite target. Use Linux-compatible release artifacts or a Linux build path when needed.

## Local setup was interrupted

Re-run the same `zodex local setup ...` command. Setup is a reconciliation path and preserves the existing Local persistent disk. A previously verified setup also has a separate last-ready recreation intent so an interrupted later attempt does not require direct edits to `~/.config/zodex`.

## Local reset refuses to run

This is often a safety preflight rather than a provider failure. `zodex local reset` will not delete the machine unless it can validate the last known-good recreation intent, required PEM/tunnel source files, GitHub App authority, network/image identity, and machine-image build prerequisites.

Restore the missing source file or run setup intentionally with replacement inputs. Do not manually delete the Apple Container machine just to bypass the reset checks.

Remember that reset is the destructive operation: a successful reset erases `/workspace`, caches, installed packages, and other persistent Local data, recreates the machine, and leaves MCP access off until a new `local start --ttl ...`.

## Local resources do not match setup

`zodex local status` reports requested-versus-observed CPU or memory drift. Re-run setup with the intended `--cpus` / `--memory` values. Machine resource changes can require a stop before the updated allocation appears on the next start; Zodex handles that reconciliation without using reset.

## Stop conditions

Stop and fix the environment before continuing when:

- reader app has permissions beyond `Contents: Read-only`
- writer app is installed too broadly
- writer app has broader permissions than `Contents` and `Pull requests`
- `zodexd` cannot bind after setup
- token minting or installation validation fails
- the agent is running as root
