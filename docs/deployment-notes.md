# Deployment Notes

This file holds the details that were intentionally kept out of the main README.

## Default Paths And Defaults

These are the main defaults:

- config file: `/etc/computer-mcp/config.toml`
- reader key: `/etc/computer-mcp/reader/private-key.pem`
- publisher key: `/etc/computer-mcp/publisher/private-key.pem`
- bind address: `0.0.0.0:443`
- agent user: `computer-mcp-agent`
- publisher user: `computer-mcp-publisher`
- publisher socket: `/var/lib/computer-mcp/publisher/run/computer-mcp-prd.sock`

Most deployments only need to change:

- `reader_app_id`
- `reader_installation_id`
- `publisher_app_id`
- `publisher_targets`

Use overrides only when you actually need them, for example a non-443 port or a custom config path.

## Install From A Private Repo Checkout

The public installer tries GitHub Release artifacts first. If no matching release asset exists, it falls back to a source build.

If the public installer URL is not usable, build from a local checkout and point the installer at the built binaries:

```bash
cargo build --release --bin computer-mcp --bin computer-mcpd --bin computer-mcp-prd
sudo COMPUTER_MCP_BINARY_SOURCE_DIR="$PWD/target/release" bash scripts/install.sh
```

## Container Hosts

Before using the standard start flow, check whether the host actually has a usable `systemd`:

```bash
ps -p 1 -o pid=,comm=,args=
which systemctl || true
systemctl is-system-running || true
```

If PID 1 is not `systemd`, `computer-mcp` uses process mode instead.

On container-style hosts:
- `computer-mcp start` runs `computer-mcp-prd` and `computer-mcpd` as detached processes
- pid and log files are stored under the state directory
- restart persistence depends on the container lifecycle, not `systemd`

## Security Model

The deployment is split into two local services:

- `computer-mcpd` runs the remote coding tools as `agent_user`
- `computer-mcp-prd` holds the GitHub App private key as `publisher_user`

`computer-mcp publish-pr` creates a local `git bundle` and sends it over a Unix socket to the publisher daemon. The agent never needs the GitHub write credential directly.

Important limits:
- do not run the coding agent as `root` if you want publisher-key isolation
- do not give the coding agent unrestricted `sudo`
- keep the publisher key readable only by `publisher_user`
- keep `publisher_targets` restricted to approved repositories

## Useful Installer Overrides

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
- `COMPUTER_MCP_READER_KEY_DIR`
- `COMPUTER_MCP_PUBLISHER_KEY_DIR`
- `COMPUTER_MCP_HTTP_BIND_PORT`
- `COMPUTER_MCP_PUBLIC_HOST`
- `COMPUTER_MCP_ENABLE_CERTBOT`

Example:

```bash
COMPUTER_MCP_VERSION=v0.1.0 \
COMPUTER_MCP_INSTALL_DIR=/usr/local/bin \
curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo -E bash
```

If you use a non-default config file, add `--config /path/to/config.toml` to the `computer-mcp` commands from the main README.
