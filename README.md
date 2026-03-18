# computer-mcp

Remote coding MCP server for Linux VPS deployment.

This README is the fast path for a fresh VPS.

Agents doing a full operator-led setup should read [docs/agent-vps-setup-runbook.md](docs/agent-vps-setup-runbook.md) first.

For extra detail, see:
- [docs/agent-vps-setup-runbook.md](docs/agent-vps-setup-runbook.md)
- [docs/deployment-notes.md](docs/deployment-notes.md)
- [docs/github-app-agent-auth.md](docs/github-app-agent-auth.md)
- [docs/runpod-deployment.md](docs/runpod-deployment.md)

If the target host is Runpod, use [docs/runpod-deployment.md](docs/runpod-deployment.md).
The main README below is the standard Linux VPS path.

## What You Need

- A Linux VPS
- `root` or `sudo`
- A public IP or host for the MCP endpoint
- A reader GitHub App private key
- A publisher GitHub App private key

Default config file: `/etc/computer-mcp/config.toml`

The commands below assume that default path. If you use a different config file, add `--config /path/to/config.toml`.

## 1. Install

If you have a public installer URL:

```bash
curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo bash
```

The installer downloads prebuilt Linux release artifacts when they are available.
It falls back to a source build only if no matching release asset exists.

If this repository is private or the raw installer URL is not accessible, use the local-source install in [docs/deployment-notes.md](docs/deployment-notes.md).

## 2. Edit Only What You Need

Edit `/etc/computer-mcp/config.toml`.

Most installs can keep the defaults. The installer already creates a strong random API key, default users, default paths, and the default HTTPS bind.

You usually only need to add the two GitHub App settings:

```toml
reader_app_id = 123456
reader_installation_id = 234567890
publisher_app_id = 3123864

[[publisher_targets]]
id = "amxv/computer-mcp"
repo = "amxv/computer-mcp"
default_base = "main"
installation_id = 117314785
```

## 3. Place Both GitHub App Keys

Default key paths:

- reader: `/etc/computer-mcp/reader/private-key.pem`
- publisher: `/etc/computer-mcp/publisher/private-key.pem`

```bash
sudo install -d -m 0750 -o root -g computer-mcp /etc/computer-mcp/reader
sudo install -m 0640 -o root -g computer-mcp \
  /path/to/reader-app.pem \
  /etc/computer-mcp/reader/private-key.pem

sudo install -m 0600 -o computer-mcp-publisher -g computer-mcp \
  /path/to/publisher-app.pem \
  /etc/computer-mcp/publisher/private-key.pem
```

## 4. Start

```bash
computer-mcp start
```

`computer-mcp start` does the rest:

- checks both GitHub Apps are configured
- creates TLS artifacts if they do not exist yet
- starts the publisher daemon
- starts the MCP daemon

The installer already generated an API key. Rotate it only if you want a new one:

```bash
computer-mcp set-key "<strong-random-key>"
```

## 5. Verify

```bash
computer-mcp status
computer-mcp show-url --host "<public_ip_or_host>"
curl -k "https://<public_ip_or_host>/health"
```

Expected MCP URL shape:

```text
https://<public_ip_or_host>/mcp?key=<api_key>
```

## 6. Open A PR From The Agent

After the agent has finished work in a local git checkout and committed the change:

```bash
computer-mcp publish-pr \
  --repo amxv/computer-mcp \
  --title "Agent: example change" \
  --body "Automated change from computer-mcp."
```

Requirements:
- run it from inside the repo checkout
- keep the worktree clean
- make sure the change is already committed on `HEAD`

## Common Commands

```bash
computer-mcp start
computer-mcp stop
computer-mcp status
computer-mcp logs
computer-mcp publisher status
computer-mcp publisher logs
computer-mcp restart
```
