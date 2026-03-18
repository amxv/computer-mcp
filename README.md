# computer-mcp

Remote coding MCP server for Linux VPS deployment.

This README is the fast path for a fresh VPS.

For extra detail, see:
- [deployment-notes.md](/Users/ashray/code/amxv/computer-mcp/docs/deployment-notes.md)
- [github-app-agent-auth.md](/Users/ashray/code/amxv/computer-mcp/docs/github-app-agent-auth.md)

## What You Need

- A Linux VPS
- `root` or `sudo`
- A public IP or host for the MCP endpoint
- A GitHub App private key if you want the agent to open PRs

Default config file: `/etc/computer-mcp/config.toml`

The commands below assume that default path. If you use a different config file, add `--config /path/to/config.toml`.

## 1. Install

If you have a public installer URL:

```bash
curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo bash
```

If this repository is private or the raw installer URL is not accessible, use the local-source install in [deployment-notes.md](/Users/ashray/code/amxv/computer-mcp/docs/deployment-notes.md).

## 2. Edit Only What You Need

Edit `/etc/computer-mcp/config.toml`.

Most installs can keep the defaults. The installer already creates a strong random API key, default users, default paths, and the default HTTPS bind.

You usually only need to add GitHub publishing settings:

```toml
publisher_app_id = 3123864

[[publisher_targets]]
id = "amxv/computer-mcp"
repo = "amxv/computer-mcp"
default_base = "main"
installation_id = 117314785
```

## 3. Place The GitHub App Key

Only required if you want `publish-pr`. The default key path is `/etc/computer-mcp/publisher/private-key.pem`.

```bash
sudo install -m 0600 -o computer-mcp-publisher -g computer-mcp \
  /path/to/github-app.pem \
  /etc/computer-mcp/publisher/private-key.pem
```

## 4. Set Up TLS

```bash
computer-mcp tls setup
```

The installer already generated an API key. Rotate it only if you want a new one:

```bash
computer-mcp set-key "<strong-random-key>"
```

## 5. Start

If you want PR publishing, start the publisher first:

```bash
computer-mcp publisher start
computer-mcp start
```

## 6. Verify

```bash
computer-mcp publisher status
computer-mcp status
computer-mcp show-url --host "<public_ip_or_host>"
curl -k "https://<public_ip_or_host>/health"
```

Expected MCP URL shape:

```text
https://<public_ip_or_host>/mcp?key=<api_key>
```

## 7. Open A PR From The Agent

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
computer-mcp status
computer-mcp logs
computer-mcp publisher status
computer-mcp publisher logs
computer-mcp restart
```
