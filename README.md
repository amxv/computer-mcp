# computer-mcp

Linux-first MCP server and CLI for remote VPS command execution with Codex-style tooling.

## One-Command Install (VPS)

Run as root (or via `sudo`) on a Linux host:

```bash
curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo bash
```

If this repository is private, that raw GitHub URL will return `404` without authenticated access. In that case, publish the installer from a public location or use one of the installer overrides to source binaries locally.

What the installer does:
- Detects distro and architecture (Ubuntu/Debian are first-class for v1).
- Installs prerequisites (`curl`, `ca-certificates`, `systemd`, plus `git`/archive tools for fallback path).
- Installs `computer-mcp`, `computer-mcpd`, and `computer-mcp-prd` binaries.
- Creates config/state directories with restricted permissions.
- Creates separate `computer-mcp-agent` and `computer-mcp-publisher` service users plus a shared `computer-mcp` group.
- Runs `computer-mcp install` to configure the service backend.
- Uses `systemd` on normal VMs and falls back to process mode on container-style environments where PID 1 is not `systemd`.
- Prints next-step commands for `set-key`, publisher setup, `tls setup`, `publisher start`, `start`, and `show-url`.

The installer is non-interactive and idempotent on re-run.

## VPS Quickstart (HTTPS-Only)

After installation, use this deploy-ready sequence:

```bash
computer-mcp --config /etc/computer-mcp/config.toml set-key "<strong-random-key>"
install -m 0600 -o computer-mcp-publisher -g computer-mcp /path/to/github-app.pem /etc/computer-mcp/publisher/private-key.pem
# then set publisher_app_id and publisher_targets in /etc/computer-mcp/config.toml
computer-mcp --config /etc/computer-mcp/config.toml tls setup
computer-mcp --config /etc/computer-mcp/config.toml publisher start
computer-mcp --config /etc/computer-mcp/config.toml start
computer-mcp --config /etc/computer-mcp/config.toml show-url --host "<vps_public_ip>"
```

Verification commands:

```bash
computer-mcp --config /etc/computer-mcp/config.toml status
computer-mcp --config /etc/computer-mcp/config.toml publisher status
curl -k "https://<vps_public_ip>/health"
```

Sample MCP URL shape:

```text
https://<vps_public_ip>/mcp?key=<your_api_key>
```

`computer-mcp` CLI output redacts `key=` query values by default to reduce accidental key leaks.

On Runpod-style container pods, `computer-mcp` uses process mode instead of `systemd`. In that mode:

- `computer-mcp start` launches `computer-mcpd` as a detached process under the configured `agent_user`
- `computer-mcp publisher start` launches `computer-mcp-prd` as a detached process under the configured `publisher_user`
- `computer-mcp stop` / `restart` / `status` / `logs` use pid and log files under the computer-mcp state directory
- `computer-mcp publish-pr` sends a local `git bundle` to `computer-mcp-prd` over a Unix socket so the agent never handles a GitHub write token
- boot persistence depends on the platform/container lifecycle rather than `systemd`

## Local PR Broker

The publish path is split in two:

- `computer-mcpd` runs remote commands as `agent_user`
- `computer-mcp-prd` holds the GitHub App private key as `publisher_user`
- `computer-mcp publish-pr --repo ... --title ...` creates a local `git bundle` from `HEAD` and submits it over the local Unix socket configured by `publisher_socket_path`

Important constraints:

- Do not run the MCP daemon as `root` if you want the publisher key to stay hidden from the agent.
- Do not grant the agent unrestricted `sudo` or arbitrary OS package install rights.
- Keep the publisher key at `publisher_private_key_path` readable only by `publisher_user`.
- Add allowed repositories in `publisher_targets`; the publisher refuses repo IDs not in that allowlist.

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
