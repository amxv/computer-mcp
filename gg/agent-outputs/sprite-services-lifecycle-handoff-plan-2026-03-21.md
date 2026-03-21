# Sprite Services Lifecycle Handoff Plan

## State of Current System

`computer-mcp` is currently deployed on the `computer` Sprite in local process mode, not as Sprite-native Services.

Observed live state on March 21, 2026:

- `GET /v1/sprites/computer/services` returned `[]`
- the public Cloudflare Worker and custom domain were healthy only when `computer-mcpd` happened to be running
- when ChatGPT agents reported hanging on simple tool calls like `pwd`, the live Sprite showed stale pid files for both:
  - `computer-mcpd.service`
  - `computer-mcp-prd`
- manual `sudo computer-mcp restart` restored service immediately
- after restart, the full MCP path worked again:
  - `initialize` -> `200`
  - `tools/list` -> `200`
  - `tools/call` for `exec_command { cmd: "pwd" }` -> success with `cwd = "/workspace"`

Important environment facts already established:

- installed binaries already exist on the Sprite:
  - `/usr/local/bin/computer-mcpd`
  - `/usr/local/bin/computer-mcp-prd`
- existing config is already correct for Sprite networking:
  - `/etc/computer-mcp/config.toml`
  - `bind_port = 8443`
  - `http_bind_port = 8080`
- security split is already correct:
  - coding daemon user: `computer-mcp-agent`
  - publisher daemon user: `computer-mcp-publisher`
  - publisher key remains `0600` and unreadable by `computer-mcp-agent`
- current public path remains:
  - Cloudflare Worker -> `computer.ashray.xyz`
  - Sprite origin -> `computer-zrmu.sprites.app`

Root cause:

- Sprites persist filesystem, but normal processes do not survive sleep/hibernation
- the current deployment depends on detached processes only
- there are no Sprite Services to auto-restart `computer-mcpd` or `computer-mcp-prd` on wake

Relevant evidence from Sprites docs:

- working-with-sprites says running processes stop when the Sprite sleeps
- it explicitly recommends Services for web servers because they auto-restart on wake
- it explicitly says Services survive hibernation and TTY sessions do not

Relevant external docs:

- https://docs.sprites.dev/working-with-sprites/
- https://docs.sprites.dev/api/v001-rc30/services/

Relevant repo files:

- `scripts/setup-sprite.sh`
- `docs/agent-sprites-setup-runbook.md`
- `docs/deployment-notes.md`
- `src/bin/computer-mcp.rs`

## State of Ideal System

The Sprite deployment should be Sprite-native:

- `computer-mcp-prd` is managed by a Sprite Service
- `computer-mcpd` is managed by a Sprite Service
- `computer-mcpd` still runs as `computer-mcp-agent`
- `computer-mcp-prd` still runs as `computer-mcp-publisher`
- the public Sprite URL and Cloudflare Worker can wake the Sprite and rely on the platform to restart the daemons automatically
- no manual `computer-mcp restart` is needed after sleep
- `tools/call` for trivial commands like `pwd` remains reliable after wake
- repo setup scripts know how to create/update Sprite Services automatically
- Sprite status/debugging can inspect Service state and Service logs, not just local pid files

This should be achieved without changing the Cloudflare Worker and without requiring a new binary release for the immediate live fix.

## Plan Phases

### Phase 1: Register Sprite Services In Place On The Live Sprite

#### Files to read before starting

- `docs/agent-sprites-setup-runbook.md`
- `docs/deployment-notes.md`
- `scripts/setup-sprite.sh`
- Sprites docs:
  - https://docs.sprites.dev/working-with-sprites/
  - https://docs.sprites.dev/api/v001-rc30/services/

#### What to do

1. Confirm the current live service list is still empty:
   - `sprite api -s computer /services`

2. Capture a pre-change checkpoint and current runtime state:
   - `sprite checkpoint create`
   - `sprite exec -- sudo computer-mcp status`
   - `sprite exec -- ps -ef | egrep "computer-mcpd|computer-mcp-prd"`

3. Define two Sprite Services using the existing installed binaries and existing config file.

Service 1: `computer-mcp-prd`

- service name: `computer-mcp-prd`
- command should launch the already installed publisher daemon
- service should preserve the dedicated publisher user boundary
- recommended command shape:

```json
{
  "cmd": "sudo",
  "args": [
    "-n",
    "-u",
    "computer-mcp-publisher",
    "/usr/local/bin/computer-mcp-prd",
    "--config",
    "/etc/computer-mcp/config.toml"
  ],
  "needs": [],
  "http_port": null
}
```

Service 2: `computer-mcpd`

- service name: `computer-mcpd`
- command should launch the already installed main daemon
- service should preserve the dedicated coding daemon user boundary
- service should expose `http_port = 8080`
- recommended command shape:

```json
{
  "cmd": "sudo",
  "args": [
    "-n",
    "-u",
    "computer-mcp-agent",
    "/usr/local/bin/computer-mcpd",
    "--config",
    "/etc/computer-mcp/config.toml"
  ],
  "needs": ["computer-mcp-prd"],
  "http_port": 8080
}
```

4. Use the Services API directly because the currently installed `sprite` CLI on this machine does not expose a `sprite services` subcommand even though the docs describe one.

Expected API shape from docs:

- list services: `GET /v1/sprites/{sprite}/services`
- create or update service: `PUT /v1/sprites/{sprite}/services/{service-name}`
- get service logs: `GET /v1/sprites/{sprite}/services/{service-name}/logs`

Expected creation pattern:

```bash
sprite api -s computer /services/computer-mcp-prd \
  -X PUT \
  -H 'Content-Type: application/json' \
  -d '{...}'

sprite api -s computer /services/computer-mcpd \
  -X PUT \
  -H 'Content-Type: application/json' \
  -d '{...}'
```

5. Stop relying on the existing detached processes before or immediately after service creation so ports are not contested.

Recommended order:

- stop current local process-mode daemons
- create/update `computer-mcp-prd`
- create/update `computer-mcpd`
- verify that the Service-managed processes are the ones bound to ports `8443` and `8080`

6. Verify Service logs are accessible through the Sprite Services API and stored under `/.sprite/logs/services/...`.

#### Validation strategy

Run all of these after creating the two Services:

1. Service inventory and state:

```bash
sprite api -s computer /services | jq .
```

2. Health checks:

```bash
curl -fsS https://computer.ashray.xyz/health
curl -fsS https://computer-zrmu.sprites.app/health
```

3. MCP initialize:

- `POST https://computer.ashray.xyz/mcp?key=...`
- expect `200` and `mcp-session-id`

4. MCP tool call:

- call `exec_command` with `cmd = "pwd"`
- expect result showing `cwd = "/workspace"`

5. Wake-cycle verification:

- allow the Sprite to idle long enough to sleep
- re-run `curl https://computer.ashray.xyz/health`
- re-run MCP `initialize`
- re-run `tools/call` for `pwd`
- confirm the services came back without manual `computer-mcp restart`

6. User separation verification:

- `computer-mcpd` effective user must still be `computer-mcp-agent`
- `computer-mcp-prd` effective user must still be `computer-mcp-publisher`
- `computer-mcp-agent` must still fail to read `/etc/computer-mcp/publisher/private-key.pem`

#### Risks / fallbacks

Risk:
- the Services runtime might not allow `sudo -n` inside service commands

Fallback:
- create a minimal local wrapper script owned by root that execs the target binary under the intended user, then point the Service at that wrapper

Risk:
- service creation may auto-start while old detached daemons still hold ports `8080` and `8443`

Fallback:
- stop the old process-mode daemons first and re-run service creation

Risk:
- the current `computer-mcp status` output remains misleading because it only understands systemd/process pidfiles

Fallback:
- treat `sprite api -s computer /services` and `.../logs` as the authoritative runtime view until repo follow-up is complete

### Phase 2: Productize Sprite Service Management In Repo Setup Flow

#### Files to read before starting

- `scripts/setup-sprite.sh`
- `docs/agent-sprites-setup-runbook.md`
- `docs/deployment-notes.md`
- `src/bin/computer-mcp.rs`

#### What to do

1. Update `scripts/setup-sprite.sh` so Sprite installs create or update the two Sprite Services automatically after binary install and config/key placement.

2. Keep the current non-root split unchanged:

- `computer-mcp-agent`
- `computer-mcp-publisher`
- existing key permissions
- existing `/workspace` workdir model

3. Add explicit service-management steps to the Sprite runbook.

4. Document that for Sprites, the runtime source of truth is now the Sprite Services API, not detached local process mode.

5. Ensure the setup script is idempotent:

- repeated runs should update service definitions, not duplicate them
- repeated runs should not require a new binary release

#### Validation strategy

- run the updated setup script against a Sprite that already has `computer-mcp` installed
- confirm service definitions converge cleanly
- confirm wake/recovery works without manual restart
- confirm docs match the real API and commands used

#### Risks / fallbacks

Risk:
- script logic may mix “install binaries” and “manage lifecycle” too tightly

Fallback:
- split service registration into a separate helper script invoked by `setup-sprite.sh`

Risk:
- future Sprite CLI versions may add native `sprite services ...` commands

Fallback:
- keep the implementation on `sprite api ...` for now, then optionally swap to first-class CLI later

### Phase 3: Teach `computer-mcp` About Sprite Service Mode

#### Files to read before starting

- `src/bin/computer-mcp.rs`
- `docs/deployment-notes.md`
- `docs/agent-sprites-setup-runbook.md`

#### What to do

1. Add a Sprite-aware status mode so operators can see Service state instead of stale local pidfile state when deployed on Sprites.

2. Do not make this depend on the current detached-process assumptions.

3. Add an explicit operational path for:

- listing services
- reading service logs
- distinguishing:
  - binary/config errors
  - Sprite service errors
  - public URL / Worker errors

4. If warranted, add a helper such as:

- `computer-mcp sprite-services-status`
- or `computer-mcp sprite-sync-services`

Only do this after the in-place fix and setup-script automation are proven.

#### Validation strategy

- verify status output stays correct when the Sprite sleeps and wakes
- verify status output matches `sprite api -s <name> /services`
- verify stale local pidfiles no longer mislead operators on Sprite deployments

#### Risks / fallbacks

Risk:
- overloading the existing generic `status` command may complicate non-Sprite providers

Fallback:
- add a Sprite-specific subcommand first, then unify later if the model proves clean

## Cross-provider requirements

The implementation must preserve provider-specific lifecycle differences.

For Sprites:
- platform lifecycle should be handled with Sprite Services
- public readiness should depend on `http_port = 8080`
- the daemon user split must remain intact

For VPS and Runpod:
- do not regress current `systemd` or process-mode flows
- do not force Sprite-specific service assumptions into non-Sprite providers

For all providers:
- `computer-mcp-agent` must remain non-root
- `computer-mcp-publisher` must remain isolated from the coding agent
- publisher private key must stay unreadable to the coding agent
- `/workspace` should remain the default non-root writable workdir where already adopted

## Recommendation

Execute Phase 1 first on the live Sprite without changing binaries. That is the fastest path to stop the hanging `pwd` symptom from recurring after Sprite sleep.

Then implement Phase 2 in the repo so future Sprite setups are correct by default.

Phase 3 is worthwhile, but only after the platform lifecycle is fixed and stable.
