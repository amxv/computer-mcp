# GitHub App Auth For The Publisher Architecture

`computer-mcp` now uses a split model for PR creation:

- the coding agent edits code, runs tests, and makes a local commit
- the local publisher daemon holds the GitHub App private key
- `computer-mcp publish-pr` sends a local `git bundle` to the publisher daemon
- the publisher daemon mints a short-lived installation token internally, pushes a generated branch, opens the PR, and returns the PR URL

The goal is simple: the agent can ask for a PR without ever holding the GitHub write credential itself.

Default config file: `/etc/computer-mcp/config.toml`

Most installs only need to add:

- `publisher_app_id`
- one or more `publisher_targets`

## What The GitHub App Is Used For

The publisher daemon uses the GitHub App to:

- push a feature branch
- create a pull request

Required repository permissions:

- `Contents: Read & write`
- `Pull requests: Read & write`
- `Metadata: Read-only`

## Manual GitHub Setup

GitHub App registration is still a manual step.

Create a private GitHub App, install it only on the repository you want to publish to, then record:

- the App ID
- the installation ID
- the path to the downloaded `.pem` private key

## Configure `computer-mcp`

Example config:

```toml
publisher_app_id = 3123864

[[publisher_targets]]
id = "amxv/computer-mcp"
repo = "amxv/computer-mcp"
default_base = "main"
installation_id = 117314785
```

Place the private key where the publisher daemon expects it:

```bash
sudo install -m 0600 -o computer-mcp-publisher -g computer-mcp \
  /path/to/github-app.pem \
  /etc/computer-mcp/publisher/private-key.pem
```

Then start the publisher daemon:

```bash
computer-mcp publisher start
computer-mcp publisher status
```

## How `publish-pr` Works

Run `publish-pr` from inside the repo checkout after the change has already been committed:

```bash
computer-mcp publish-pr \
  --repo amxv/computer-mcp \
  --title "Agent: example change" \
  --body "Automated change from computer-mcp."
```

Current requirements:

- the current directory must be inside a git repo
- the worktree must be clean
- the commit you want in the PR must already be on `HEAD`
- the `--repo` value must match one of the configured `publisher_targets`

`publish-pr` does not expose or print the GitHub installation token.

## What This Does And Does Not Protect

This architecture protects the GitHub write credential from the coding agent only if the agent is not running with unrestricted root-level access.

Good:
- `computer-mcpd` runs as `computer-mcp-agent`
- `computer-mcp-prd` runs as `computer-mcp-publisher`
- the publisher key is readable only by `computer-mcp-publisher`

Bad:
- the coding agent runs as `root`
- the coding agent has unrestricted `sudo`
- the coding agent can read the publisher user's files or processes

## Private Repo Branch Protection Note

On a private personal GitHub repo without GitHub Pro, GitHub will not enforce protected branches server-side.

With this architecture, the main safety property does not come from GitHub blocking `main`. It comes from keeping the GitHub write credential inside the publisher daemon instead of handing it to the coding agent.

If the coding agent also needs to clone private repos directly, give it a separate read-only credential or prepare the checkout another way. This document only covers the publisher write-auth path.
