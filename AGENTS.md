# AGENTS.md

## Runpod VPS / Pod Notes

This repository can be deployed to Runpod, but agents need to account for how Runpod exposes SSH and how many pods are actually container environments rather than full VM environments.

## SSH Access

Runpod commonly exposes two different SSH paths:

1. `Basic SSH` through the `ssh.runpod.io` gateway
2. `Full SSH` through the pod's public IP and mapped TCP port

For automation and one-shot remote commands, prefer `Full SSH`.

Use this shape:

```bash
ssh -p <runpod_tcp_port_22> root@<runpod_public_ip> -i ~/.ssh/id_ed25519 '<command>'
```

Do not rely on the `ssh.runpod.io` gateway for non-interactive command execution. In practice, the gateway may force a login shell / PTY flow and ignore the remote command payload.

If only the gateway command is known, connect once and inspect the environment to discover the direct endpoint:

```bash
env | sort | grep -E 'RUNPOD_PUBLIC_IP|RUNPOD_TCP_PORT_22'
```

Expected variables:

```bash
RUNPOD_PUBLIC_IP=<public_ip>
RUNPOD_TCP_PORT_22=<ssh_port>
```

## Environment Detection

Before using the current `computer-mcp install` / `start` flow on Runpod, check whether the pod actually has a usable `systemd`:

```bash
ps -p 1 -o pid=,comm=,args=
which systemctl || true
systemctl is-system-running || true
```

If PID 1 is something like `/bin/bash /start.sh` and `systemctl is-system-running` returns `offline`, treat the pod as a container-style environment, not a normal systemd VM.

## Current Repo Limitation

The current `computer-mcp` CLI service management flow assumes `systemd` is usable:

- `computer-mcp install`
- `computer-mcp start`
- `computer-mcp stop`
- `computer-mcp restart`
- `computer-mcp status`
- `computer-mcp logs`

On Runpod pods where `systemd` is offline, that flow is not compatible as-is.

Agents should detect this first and avoid pretending the standard install path will work.

## Public Endpoint Requirement

A working SSH connection is not enough to expose the MCP server publicly.

Agents must also verify that the pod has a public TCP port mapped for the MCP HTTPS listener. If only SSH is exposed publicly, then:

- the daemon may still start locally
- but `https://<public_ip>:<mapped_port>/mcp?key=...` will not be reachable from outside

Before claiming deployment is complete, verify that:

1. a public TCP port is mapped for the MCP server
2. the app is configured to bind to that port
3. the endpoint is reachable externally

## Recommended Runpod Workflow

For Runpod pods, use this sequence:

1. Discover the direct SSH endpoint (`RUNPOD_PUBLIC_IP`, `RUNPOD_TCP_PORT_22`).
2. Use direct SSH for all non-interactive commands.
3. Detect whether `systemd` is usable.
4. If `systemd` is offline, use a container-compatible process strategy instead of the current systemd flow.
5. Confirm the MCP port is publicly exposed before validating the final URL.

## URL Shape

The intended public MCP URL still follows this shape:

```text
https://<public_ip_or_host>:<public_port>/mcp?key=<apikey>
```

If the deployment target is a normal VM with working `systemd`, the existing CLI flow is appropriate.

If the deployment target is a Runpod container-style pod, agents should assume additional adaptation is required.
