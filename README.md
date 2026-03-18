# computer-mcp

Remote coding MCP server for Linux VPS deployment.

This README is the fast path for deploying the software on a VPS.

For detailed notes, see:
- [deployment-notes.md](/Users/ashray/code/amxv/computer-mcp/docs/deployment-notes.md)
- [github-app-agent-auth.md](/Users/ashray/code/amxv/computer-mcp/docs/github-app-agent-auth.md)

## 1. Install

Run as `root` or with `sudo`:

```bash
curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | sudo bash
```

If this repository is private, that raw GitHub URL will return `404` without authenticated access. In that case, publish the installer somewhere public or use the installer overrides documented in [deployment-notes.md](/Users/ashray/code/amxv/computer-mcp/docs/deployment-notes.md).

## 2. Configure

Edit `/etc/computer-mcp/config.toml` and set the important values.

Minimum example:

```toml
bind_host = "0.0.0.0"
bind_port = 443
api_key = "replace-me"
publisher_app_id = 3123864
agent_user = "computer-mcp-agent"
publisher_user = "computer-mcp-publisher"
service_group = "computer-mcp"

[[publisher_targets]]
id = "amxv/computer-mcp"
repo = "amxv/computer-mcp"
default_base = "main"
installation_id = 117314785
```

Place the GitHub App private key at the configured publisher key path:

```bash
install -m 0600 -o computer-mcp-publisher -g computer-mcp \
  /path/to/github-app.pem \
  /etc/computer-mcp/publisher/private-key.pem
```

## 3. Start Services

Run these in order:

```bash
computer-mcp --config /etc/computer-mcp/config.toml set-key "<strong-random-key>"
computer-mcp --config /etc/computer-mcp/config.toml tls setup
computer-mcp --config /etc/computer-mcp/config.toml publisher start
computer-mcp --config /etc/computer-mcp/config.toml start
computer-mcp --config /etc/computer-mcp/config.toml show-url --host "<public_ip>"
```

On container-style hosts like Runpod, the CLI automatically uses process mode instead of `systemd` when PID 1 is not `systemd`.

## 4. Verify

```bash
computer-mcp --config /etc/computer-mcp/config.toml publisher status
computer-mcp --config /etc/computer-mcp/config.toml status
curl -k "https://<public_ip>/health"
```

Expected MCP URL shape:

```text
https://<public_ip>/mcp?key=<your_api_key>
```

## 5. Agent PR Workflow

After the agent has finished work in a local git checkout and committed the change, it can open a PR through the local publisher daemon:

```bash
computer-mcp --config /etc/computer-mcp/config.toml publish-pr \
  --repo amxv/computer-mcp \
  --title "Agent: example change" \
  --body "Automated change from computer-mcp."
```

Requirements:
- current directory must be a git checkout
- the worktree must be clean
- the desired change must already be committed to `HEAD`

## 6. Common Commands

```bash
computer-mcp --config /etc/computer-mcp/config.toml status
computer-mcp --config /etc/computer-mcp/config.toml logs
computer-mcp --config /etc/computer-mcp/config.toml publisher status
computer-mcp --config /etc/computer-mcp/config.toml publisher logs
computer-mcp --config /etc/computer-mcp/config.toml restart
```
