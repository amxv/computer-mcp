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
- Runs `computer-mcp install` to configure the service backend.
- Uses `systemd` on normal VMs and falls back to process mode on container-style environments where PID 1 is not `systemd`.
- Prints next-step commands for `set-key`, `tls setup`, `start`, and `show-url`.

The installer is non-interactive and idempotent on re-run.

## VPS Quickstart (HTTPS-Only)

After installation, use this deploy-ready sequence:

```bash
computer-mcp --config /etc/computer-mcp/config.toml set-key "<strong-random-key>"
computer-mcp --config /etc/computer-mcp/config.toml tls setup
computer-mcp --config /etc/computer-mcp/config.toml start
computer-mcp --config /etc/computer-mcp/config.toml show-url --host "<vps_public_ip>"
```

Verification commands:

```bash
computer-mcp --config /etc/computer-mcp/config.toml status
curl -k "https://<vps_public_ip>/health"
```

Sample MCP URL shape:

```text
https://<vps_public_ip>/mcp?key=<your_api_key>
```

`computer-mcp` CLI output redacts `key=` query values by default to reduce accidental key leaks.

On Runpod-style container pods, `computer-mcp` uses process mode instead of `systemd`. In that mode:

- `computer-mcp start` launches `computer-mcpd` as a detached process
- `computer-mcp stop` / `restart` / `status` / `logs` use pid and log files under the computer-mcp state directory
- boot persistence depends on the platform/container lifecycle rather than `systemd`

## GitHub App Agent Auth

For repository-scoped agent push/branch/PR access without placing a broad personal token on a VPS, see [docs/github-app-agent-auth.md](/Users/ashray/code/amxv/computer-mcp/docs/github-app-agent-auth.md).

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
