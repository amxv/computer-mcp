# Deployment Notes

This file contains the less important details that were intentionally moved out of the main README.

## Runpod / Container Hosts

If PID 1 is not `systemd`, `computer-mcp` uses process mode.

Useful checks:

```bash
ps -p 1 -o pid=,comm=,args=
which systemctl || true
systemctl is-system-running || true
```

On Runpod-style pods:
- `computer-mcp start` launches `computer-mcpd` as a detached process under `agent_user`
- `computer-mcp publisher start` launches `computer-mcp-prd` as a detached process under `publisher_user`
- pid/log files are stored under the computer-mcp state directory
- boot persistence depends on the container lifecycle rather than `systemd`

## Security Model

The intended split is:
- `computer-mcpd` runs remote commands as `agent_user`
- `computer-mcp-prd` holds the GitHub App private key as `publisher_user`
- `computer-mcp publish-pr` creates a local `git bundle` and sends it over a Unix socket to the publisher daemon

Important constraints:
- do not run the agent daemon as `root` if you want publisher-key isolation
- do not give the agent unrestricted `sudo`
- do not give the agent generic root-level package-manager access
- keep the publisher key readable only by `publisher_user`
- keep `publisher_targets` restricted to allowed repositories

## Installer Overrides

`scripts/install.sh` supports these environment overrides:

- `COMPUTER_MCP_VERSION`
- `COMPUTER_MCP_REPO`
- `COMPUTER_MCP_ASSET_URL`
- `COMPUTER_MCP_SOURCE_REF`
- `COMPUTER_MCP_BINARY_SOURCE_DIR`
- `COMPUTER_MCP_INSTALL_DIR`
- `COMPUTER_MCP_CONFIG_PATH`
- `COMPUTER_MCP_STATE_DIR`
- `COMPUTER_MCP_TLS_DIR`
- `COMPUTER_MCP_AGENT_USER`
- `COMPUTER_MCP_PUBLISHER_USER`
- `COMPUTER_MCP_SERVICE_GROUP`
- `COMPUTER_MCP_PUBLISHER_KEY_DIR`
- `COMPUTER_MCP_ENABLE_CERTBOT`

Example:

```bash
COMPUTER_MCP_VERSION=v0.1.0 \
COMPUTER_MCP_INSTALL_DIR=/usr/local/bin \
curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo -E bash
```

## Private Repository Installer Note

If this repository is private, `raw.githubusercontent.com/.../scripts/install.sh` will return `404` without authenticated access.

In that case, use one of these options:
- publish the installer from a public location
- use `COMPUTER_MCP_BINARY_SOURCE_DIR` with local binaries
- provide an authenticated/private distribution path for the installer and release assets
