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

## Current Runpod Behavior

This repository now supports Runpod-style container environments without `systemd`.

When PID 1 is not `systemd`, the CLI falls back to process mode:

- `computer-mcp start|stop|restart|status|logs`
- `computer-mcp publisher start|stop|status|logs`

In that mode:

- `computer-mcpd` is launched as the configured `agent_user`
- `computer-mcp-prd` is launched as the configured `publisher_user`
- pid/log files are written under the computer-mcp state directory
- the publisher listens only on a local Unix socket, not a public TCP port

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
4. If `systemd` is offline, use the built-in process mode and verify both `computer-mcp status` and `computer-mcp publisher status`.
5. Confirm the MCP port is publicly exposed before validating the final URL.

## Security Notes

For the publisher-key isolation model to be real on Runpod:

- do not run the agent daemon as `root`
- do not give the agent unrestricted `sudo`
- do not give the agent generic root-level package-manager access
- keep the publisher GitHub App key readable only by `publisher_user`

The intended split is:

- `computer-mcpd` under `agent_user`
- `computer-mcp-prd` under `publisher_user`
- `computer-mcp publish-pr` as the narrow handoff path between them

## URL Shape

The intended public MCP URL still follows this shape:

```text
https://<public_ip_or_host>:<public_port>/mcp?key=<apikey>
```

If the deployment target is a normal VM with working `systemd`, the existing CLI flow is still appropriate.

If the deployment target is a Runpod container-style pod, use process mode and verify the two-daemon split before claiming the deployment is secure.
