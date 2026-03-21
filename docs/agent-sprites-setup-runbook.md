# computer-mcp Agent Sprites Setup Runbook

This runbook is for an agent helping a human set up `computer-mcp` on a Sprite.

Use this when the target environment is Sprites (`sprite` CLI), not a traditional VPS over SSH.

For traditional VPS setup, use [agent-vps-setup-runbook.md](agent-vps-setup-runbook.md).
For Runpod-specific rollout behavior, use [../.agents/skills/runpod-deployment/SKILL.md](../.agents/skills/runpod-deployment/SKILL.md).

## Outcome

When this runbook is complete:

- latest `computer-mcp` is installed in the target Sprite
- reader + publisher GitHub App auth is configured
- publisher and MCP daemons are running in Sprite-safe process mode
- the coding agent starts in a writable non-root workspace (`/workspace`)
- MCP endpoint is reachable through the Sprite URL

## Why Sprites Need A Slightly Different Path

Sprites are Linux boxes, but this runtime commonly uses process mode and non-root service users.

To avoid privileged port binding failures in process mode, this runbook uses:

- `bind_port = 8443` for TLS listener
- `http_bind_port = 8080` for Sprite URL routing

To avoid collapsing the security boundary around the publisher key, this runbook does not run the coding daemon as `sprite`.

Instead it uses:

- `computer-mcp-agent` as a normal non-root workspace user
- `computer-mcp-publisher` as the isolated publisher user
- `/home/computer-mcp-agent` as the agent home
- `/workspace` as the default writable workdir

This matters on Sprites because the built-in `sprite` user commonly has passwordless `sudo`. Running the coding daemon as `sprite` would effectively give the agent root and let it break publisher-key isolation.

## Required Inputs

- Sprite name (example: `computer`)
- optional organization name
- target repo slug (example: `owner/repo`)
- reader app ID
- absolute local path to reader PEM
- publisher app ID
- absolute local path to publisher PEM

Do not ask the human for installation IDs manually. Derive them.

## Fast Path (Recommended)

Use the repo script:

[`scripts/setup-sprite.sh`](../scripts/setup-sprite.sh)

Example:

```bash
scripts/setup-sprite.sh \
  --sprite computer \
  --repo amxv/computer-mcp \
  --reader-app-id <reader-app-id> \
  --reader-pem /absolute/path/to/reader-private-key.pem \
  --publisher-app-id <publisher-app-id> \
  --publisher-pem /absolute/path/to/publisher-private-key.pem \
  --default-base main \
  --url-auth sprite
```

If the Sprite is in a non-default org, add:

```bash
--org <org-name>
```

What the script does:

1. derives reader and publisher installation IDs from app ID + PEM + repo
2. validates both apps with `scripts/mint-gh-app-installation-token.sh`
3. installs latest `computer-mcp` in the Sprite
4. installs key files at default paths
5. writes a managed GitHub app config block
6. enforces Sprite-safe ports (`8443` TLS + `8080` HTTP)
7. enforces agent workspace defaults (`agent_home = "/home/computer-mcp-agent"`, `default_workdir = "/workspace"`)
8. restarts stack, verifies health, and verifies that `computer-mcp-agent` can write in `/workspace`
9. prints MCP URL hint based on Sprite URL host

## Manual Path (If You Need It)

If you cannot use the script, follow the same sequence manually:

1. Derive installation IDs using JWT + GitHub `/repos/<repo>/installation`.
2. Validate both apps with `scripts/mint-gh-app-installation-token.sh`.
3. Run installer inside Sprite:
   - `curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo env COMPUTER_MCP_HTTP_BIND_PORT=8080 COMPUTER_MCP_AGENT_HOME=/home/computer-mcp-agent COMPUTER_MCP_DEFAULT_WORKDIR=/workspace bash`
4. Install PEMs to:
   - `/etc/computer-mcp/reader/private-key.pem`
   - `/etc/computer-mcp/publisher/private-key.pem`
5. Set config:
   - `bind_port = 8443`
   - `http_bind_port = 8080`
   - `agent_home = "/home/computer-mcp-agent"`
   - `default_workdir = "/workspace"`
   - reader app fields
   - publisher app fields and target repo
6. `sudo computer-mcp restart || sudo computer-mcp start`
7. Verify:
   - `sudo computer-mcp status`
   - `sudo curl -k https://127.0.0.1:8443/health`
   - `sudo curl http://127.0.0.1:8080/health`
   - `sudo -u computer-mcp-agent env HOME=/home/computer-mcp-agent bash -lc 'cd /workspace && touch .ok && rm -f .ok'`

## Verification Checklist

- `computer-mcp status` shows:
  - `computer-mcpd.service` active
  - `computer-mcp-prd` active
  - `agent-home: /home/computer-mcp-agent`
  - `default-workdir: /workspace`
  - reader config ready
- config file contains expected app IDs and installation IDs
- reader and publisher PEM permissions are correct
- `computer-mcp-agent` can write inside `/workspace`
- Sprite URL auth mode is intentional (`sprite` by default; `public` only if required)

## Stop Conditions

Stop and ask before continuing if:

- reader app has any write permission
- publisher app has permissions beyond `contents:write` and `pull_requests:write`
- app installation scope is broader than intended
- `computer-mcpd` cannot bind even after Sprite-safe ports are set
- app token minting validation fails
