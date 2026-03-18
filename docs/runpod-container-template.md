# Runpod Container Template

Use this path when you want a prebuilt Runpod Pod image instead of starting from a generic Ubuntu container and bootstrapping the toolchain on first launch.

This image is designed for `computer-mcp` development and VPS-style operation:

- `computer-mcp`, `computer-mcpd`, and `computer-mcp-prd` are preinstalled
- Node.js, Python, Go, and Rust are preinstalled
- Git, Git LFS, GitHub CLI, SSH, and common Unix CLI tools are preinstalled
- the container injects Runpod SSH keys from `PUBLIC_KEY` / `SSH_PUBLIC_KEY`
- the container starts `sshd` automatically so a Runpod Pod can expose TCP `22`
- the container can auto-configure and auto-start `computer-mcp` from environment variables

## Image Release Pipeline

GitHub Actions publishes the image to GitHub Container Registry from:

- [container-release.yml](../.github/workflows/container-release.yml)

Published image name:

```text
ghcr.io/amxv/computer-mcp
```

Expected tags:

- `latest`
- `vX.Y.Z`
- `X.Y.Z`
- `X.Y`
- `sha-<shortsha>`

## Important GHCR Visibility Note

GitHub’s container registry docs state that a newly published package defaults to private, even when the repository is public.

That means the first publish usually needs a one-time visibility change in GitHub:

1. Open the package page under the repository owner’s **Packages** section.
2. Open the container package for `computer-mcp`.
3. Change package visibility to **Public**.

After that, Runpod can pull the image anonymously as a public image.

Relevant GitHub docs:

- `https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry`
- `https://docs.github.com/en/packages/learn-github-packages/configuring-a-packages-access-control-and-visibility`

## Recommended Runpod Template Settings

These values align with the current Runpod template docs.

Template basics:

- **Name:** `computer-mcp-dev`
- **Image name:** `ghcr.io/amxv/computer-mcp:latest`
- **Visibility:** private for your account/team, or public if you want to share the template
- **Container start command:** leave blank to use the image entrypoint

Recommended environment variables for full automatic startup:

- `COMPUTER_MCP_AUTO_START=1`
- `COMPUTER_MCP_HTTP_BIND_PORT=8080`
- `COMPUTER_MCP_PUBLIC_HOST=<pod-id>-8080.proxy.runpod.net`
- `COMPUTER_MCP_API_KEY=<strong-random-key>`
- `COMPUTER_MCP_READER_APP_ID=<reader_app_id>`
- `COMPUTER_MCP_READER_INSTALLATION_ID=<reader_installation_id>`
- `COMPUTER_MCP_READER_PRIVATE_KEY={{ RUNPOD_SECRET_reader_private_key }}`
- `COMPUTER_MCP_PUBLISHER_APP_ID=<publisher_app_id>`
- `COMPUTER_MCP_PUBLISHER_INSTALLATION_ID=<publisher_installation_id>`
- `COMPUTER_MCP_PUBLISHER_TARGET_REPO=amxv/computer-mcp`
- `COMPUTER_MCP_PUBLISHER_DEFAULT_BASE=main`
- `COMPUTER_MCP_PUBLISHER_PRIVATE_KEY={{ RUNPOD_SECRET_publisher_private_key }}`

Storage:

- **Container disk:** `40 GB`
- **Volume disk:** `20 GB`
- **Volume mount path:** `/workspace`

Ports:

- **HTTP ports:** `8080/http`
- **TCP ports:** `22/tcp`

Compute type:

- choose **CPU** for a lightweight coding Pod
- choose **NVIDIA GPU** if you want a GPU-backed dev Pod with the same image

## Creating The Template In Runpod

The Runpod template docs describe templates as a combination of:

- container image
- ports
- storage
- environment variables
- startup command

You can create the template either in the web console or with the REST API.

Example REST payload shape, adapted to this image:

```json
{
  "category": "CPU",
  "containerDiskInGb": 40,
  "dockerEntrypoint": [],
  "dockerStartCmd": [],
  "env": {
    "COMPUTER_MCP_AUTO_START": "1",
    "COMPUTER_MCP_HTTP_BIND_PORT": "8080",
    "COMPUTER_MCP_PUBLIC_HOST": "<pod-id>-8080.proxy.runpod.net",
    "COMPUTER_MCP_API_KEY": "<strong-random-key>",
    "COMPUTER_MCP_READER_APP_ID": "<reader_app_id>",
    "COMPUTER_MCP_READER_INSTALLATION_ID": "<reader_installation_id>",
    "COMPUTER_MCP_READER_PRIVATE_KEY": "{{ RUNPOD_SECRET_reader_private_key }}",
    "COMPUTER_MCP_PUBLISHER_APP_ID": "<publisher_app_id>",
    "COMPUTER_MCP_PUBLISHER_INSTALLATION_ID": "<publisher_installation_id>",
    "COMPUTER_MCP_PUBLISHER_TARGET_REPO": "amxv/computer-mcp",
    "COMPUTER_MCP_PUBLISHER_DEFAULT_BASE": "main",
    "COMPUTER_MCP_PUBLISHER_PRIVATE_KEY": "{{ RUNPOD_SECRET_publisher_private_key }}"
  },
  "imageName": "ghcr.io/amxv/computer-mcp:latest",
  "isPublic": false,
  "isServerless": false,
  "name": "computer-mcp-dev",
  "ports": [
    "8080/http",
    "22/tcp"
  ],
  "readme": "computer-mcp development pod with preinstalled toolchains",
  "volumeInGb": 20,
  "volumeMountPath": "/workspace"
}
```

## After Pod Launch

Once the Pod is up, the intended path is:

1. Runpod injects your SSH public key
2. the image starts `sshd`
3. the image writes the two GitHub App private keys from Runpod secrets
4. the image writes `/etc/computer-mcp/config.toml` from env vars
5. the image runs `computer-mcp start`

If you omit the `COMPUTER_MCP_*` environment variables, the image still comes up for SSH access, but `computer-mcp` will not auto-start.

If you just want to refresh binaries on an existing Pod that uses this image:

```bash
computer-mcp --version
computer-mcp upgrade --version v0.1.9
```

## Source Notes

This document follows the current public docs for:

- Runpod Pod templates overview and management
- GitHub Actions publishing to GHCR
- GitHub Container Registry repository linking and package visibility
