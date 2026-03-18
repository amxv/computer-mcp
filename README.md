# computer-mcp

Remote coding MCP server for Linux VPS deployment.

This README is the fast path. It shows the shortest install and run flow for a fresh VPS.

For extra detail, see:
- [deployment-notes.md](/Users/ashray/code/amxv/computer-mcp/docs/deployment-notes.md)
- [github-app-agent-auth.md](/Users/ashray/code/amxv/computer-mcp/docs/github-app-agent-auth.md)

## What You Need

- A Linux VPS
- `root` or `sudo`
- A public IP or host for the MCP endpoint
- A GitHub App private key if you want the agent to open PRs

## 1. Install

If you have a public installer URL:

```bash
curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo bash
```

If this repository is private or the raw installer URL is not accessible, use the local-source install in [deployment-notes.md](/Users/ashray/code/amxv/computer-mcp/docs/deployment-notes.md).

## 2. Edit The Config

Edit `/etc/computer-mcp/config.toml`.

Minimum example:

```toml
bind_host = "0.0.0.0"
bind_port = 443
api_key = "change-me"
publisher_app_id = 3123864

[[publisher_targets]]
id = "amxv/computer-mcp"
repo = "amxv/computer-mcp"
default_base = "main"
installation_id = 117314785
```

## 3. Place The GitHub App Key

Only required if you want `publish-pr`.

```bash
sudo install -m 0600 -o computer-mcp-publisher -g computer-mcp \
  /path/to/github-app.pem \
  /etc/computer-mcp/publisher/private-key.pem
```

## 4. Set The API Key And TLS

```bash
computer-mcp --config /etc/computer-mcp/config.toml set-key "<strong-random-key>"
computer-mcp --config /etc/computer-mcp/config.toml tls setup
```

## 5. Start The Services

```bash
computer-mcp --config /etc/computer-mcp/config.toml publisher start
computer-mcp --config /etc/computer-mcp/config.toml start
```

## 6. Verify

```bash
computer-mcp --config /etc/computer-mcp/config.toml publisher status
computer-mcp --config /etc/computer-mcp/config.toml status
computer-mcp --config /etc/computer-mcp/config.toml show-url --host "<public_ip_or_host>"
curl -k "https://<public_ip_or_host>/health"
```

Expected MCP URL shape:

```text
https://<public_ip_or_host>/mcp?key=<api_key>
```

## 7. Open A PR From The Agent

After the agent has finished work in a local git checkout and committed the change:

```bash
computer-mcp --config /etc/computer-mcp/config.toml publish-pr \
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
computer-mcp --config /etc/computer-mcp/config.toml status
computer-mcp --config /etc/computer-mcp/config.toml logs
computer-mcp --config /etc/computer-mcp/config.toml publisher status
computer-mcp --config /etc/computer-mcp/config.toml publisher logs
computer-mcp --config /etc/computer-mcp/config.toml restart
```
