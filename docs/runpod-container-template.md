# Runpod Container Template

Use this path when you want a dedicated Runpod Pod image rather than the generic VPS image.

This image is intentionally separate from the standard container path:

- [Dockerfile](../Dockerfile) is the generic image for standard Linux VPS and container environments
- [Dockerfile.runpod](../Dockerfile.runpod) is the Runpod-specific image

The Runpod image follows the structure recommended in the official Runpod template docs and the `runpod-workers/pod-template` example:

- it extends the Runpod base image instead of reimplementing Pod SSH lifecycle from scratch
- it lets the base image start Runpod services via `/start.sh`
- it runs a separate `computer-mcp` bootstrap step after those services come up
- it keeps the Pod alive for SSH/debugging if `computer-mcp` startup fails

## Image Release Pipeline

GitHub Actions publishes two GHCR packages from:

- [container-release.yml](../.github/workflows/container-release.yml)

Package names:

```text
ghcr.io/amxv/computer-mcp
ghcr.io/amxv/computer-mcp-runpod
```

Use the Runpod-specific package for templates:

```text
ghcr.io/amxv/computer-mcp-runpod
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
2. Open the container package for `computer-mcp-runpod`.
3. Change package visibility to **Public**.

After that, Runpod can pull the image anonymously as a public image.

Relevant GitHub docs:

- `https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry`
- `https://docs.github.com/en/packages/learn-github-packages/configuring-a-packages-access-control-and-visibility`

## How This Image Boots

The image is based on `runpod/base:0.6.3-cpu`, which provides the Runpod Pod startup model.

At container startup:

1. the Runpod base image handles Pod services through `/start.sh`
2. [docker/runpod-run.sh](../docker/runpod-run.sh) starts `/start.sh` in the background
3. [docker/runpod-bootstrap.sh](../docker/runpod-bootstrap.sh) creates the service accounts needed by `computer-mcp`
4. it writes secrets and config from `COMPUTER_MCP_*` env vars
5. it fixes ownership and modes for config, key, and TLS paths
6. it runs `computer-mcp start`

This keeps the Runpod-specific logic focused on `computer-mcp` itself instead of trying to own SSH/bootstrap behavior that the Runpod base image already knows how to handle.

## Recommended Runpod Template Settings

These values align with the current Runpod template docs.

Template basics:

- **Name:** `computer-mcp-dev`
- **Image name:** `ghcr.io/amxv/computer-mcp-runpod:latest`
- **Visibility:** private for your account/team, or public if you want to share the template
- **Container start command:** leave blank to use the image default `CMD`

Storage:

- **Container disk:** `40 GB`
- **Volume disk:** `20 GB`
- **Volume mount path:** `/workspace`

Ports:

- **HTTP ports:** `8080/http`
- **TCP ports:** `22/tcp`
- **TCP 443:** optional debug-only direct access

Compute type:

- choose **CPU** for a lightweight coding Pod

## Recommended Environment Variables

Preferred full-config path:

- `COMPUTER_MCP_AUTO_START=1`
- `COMPUTER_MCP_FORCE_RECONFIGURE=1`
- `COMPUTER_MCP_PUBLIC_HOST=<pod-id>-8080.proxy.runpod.net`
- `COMPUTER_MCP_CONFIG_TOML={{ RUNPOD_SECRET_computer_mcp_config_toml }}`
- `COMPUTER_MCP_READER_PRIVATE_KEY={{ RUNPOD_SECRET_reader_private_key }}`
- `COMPUTER_MCP_PUBLISHER_PRIVATE_KEY={{ RUNPOD_SECRET_publisher_private_key }}`

Alternative per-field config path:

- `COMPUTER_MCP_AUTO_START=1`
- `COMPUTER_MCP_FORCE_RECONFIGURE=1`
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

If `COMPUTER_MCP_CONFIG_TOML` is present, the container writes that file directly to `/etc/computer-mcp/config.toml`.

If `COMPUTER_MCP_PUBLIC_HOST` is omitted and Runpod provides `RUNPOD_POD_ID`, the bootstrap script derives:

```text
<pod-id>-8080.proxy.runpod.net
```

## Creating The Template In Runpod

Runpod templates are a combination of:

- container image
- ports
- storage
- environment variables
- startup command

Example REST payload shape adapted to this image:

```json
{
  "category": "CPU",
  "containerDiskInGb": 40,
  "dockerEntrypoint": [],
  "dockerStartCmd": [],
  "env": {
    "COMPUTER_MCP_AUTO_START": "1",
    "COMPUTER_MCP_FORCE_RECONFIGURE": "1",
    "COMPUTER_MCP_PUBLIC_HOST": "<pod-id>-8080.proxy.runpod.net",
    "COMPUTER_MCP_CONFIG_TOML": "{{ RUNPOD_SECRET_computer_mcp_config_toml }}",
    "COMPUTER_MCP_READER_PRIVATE_KEY": "{{ RUNPOD_SECRET_reader_private_key }}",
    "COMPUTER_MCP_PUBLISHER_PRIVATE_KEY": "{{ RUNPOD_SECRET_publisher_private_key }}"
  },
  "imageName": "ghcr.io/amxv/computer-mcp-runpod:latest",
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

## Source Notes

This document follows the current public docs for:

- Runpod Pod templates overview and management
- Runpod custom template creation
- the `runpod-workers/pod-template` example repository
- GitHub Actions publishing to GHCR
