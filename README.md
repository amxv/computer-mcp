# computer-mcp

Linux-first MCP server and CLI for remote VPS command execution with Codex-style tooling.

## One-Command Install (VPS)

Run as root (or via `sudo`) on a Linux host:

```bash
curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo bash
```

What the installer does:
- Detects distro and architecture (Ubuntu/Debian are first-class for v1).
- Installs prerequisites (`curl`, `ca-certificates`, `systemd`, plus `git`/archive tools for fallback path).
- Installs `computer-mcp` and `computer-mcpd` binaries.
- Creates config/state directories with restricted permissions.
- Runs `computer-mcp install` to configure and enable the systemd service.
- Prints next-step commands for `set-key`, `start`, and `show-url`.

The installer is non-interactive and idempotent on re-run.

## Installer Environment Overrides

`scripts/install.sh` supports the following optional overrides:

- `COMPUTER_MCP_VERSION`
  - Release version to fetch (default: `latest`).
- `COMPUTER_MCP_REPO`
  - GitHub repo in `owner/name` format (default: `amxv/computer-mcp`).
- `COMPUTER_MCP_ASSET_URL`
  - Full release artifact URL to download directly (overrides release lookup).
- `COMPUTER_MCP_SOURCE_REF`
  - Git ref used for source-build fallback (default: `main`).
- `COMPUTER_MCP_BINARY_SOURCE_DIR`
  - Local directory containing prebuilt `computer-mcp` and `computer-mcpd` binaries.
- `COMPUTER_MCP_INSTALL_DIR`
  - Destination for binaries (default: `/usr/local/bin`).
- `COMPUTER_MCP_CONFIG_PATH`
  - Config file path (default: `/etc/computer-mcp/config.toml`).
- `COMPUTER_MCP_STATE_DIR`
  - State directory (default: `/var/lib/computer-mcp`).
- `COMPUTER_MCP_TLS_DIR`
  - TLS directory (default: `${COMPUTER_MCP_STATE_DIR}/tls`).
- `COMPUTER_MCP_ENABLE_CERTBOT`
  - Set to `1` to attempt optional `certbot` install during bootstrap.

Example with explicit release and install path:

```bash
COMPUTER_MCP_VERSION=v0.1.0 \
COMPUTER_MCP_INSTALL_DIR=/usr/local/bin \
curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo -E bash
```
