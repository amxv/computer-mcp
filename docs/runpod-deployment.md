# Runpod Deployment

Use this guide when the target host is a Runpod pod instead of a normal VPS.

If you want a prebuilt Runpod image with the toolchains already installed, see [runpod-container-template.md](runpod-container-template.md). The dedicated Runpod image is published as `ghcr.io/amxv/computer-mcp-runpod`.

Runpod is different in two ways that matter here:

1. many pods do not run a usable `systemd`, so `computer-mcp` uses process mode
2. the best public URL for ChatGPT is the Runpod proxy hostname on standard `443`, not a random direct TCP port

This guide is the exact path that was validated against a real Runpod pod.

## 1. Configure Pod Networking

In the Runpod UI:

1. set `Expose HTTP ports` to `8080`
2. set `Expose TCP ports` to `22`
3. treat TCP `443` as optional debug-only access, not the preferred ChatGPT URL

After the pod restarts, collect:

- the direct SSH host
- the direct SSH port for container port `22`
- the Runpod proxy hostname, which looks like:
  - `https://<pod-id>-8080.proxy.runpod.net`

## 2. Create The GitHub Apps

Use the generic app setup flow from [agent-vps-setup-runbook.md](agent-vps-setup-runbook.md).

You need both apps:

- a reader app
- a publisher app

You also need both installation IDs for the target repo before you write the VPS config.

## 3. Set Local Variables

On your local machine:

```bash
export VPS_HOST="<runpod_public_ip>"
export VPS_PORT="<runpod_ssh_port>"
export VPS_USER="root"
export VPS_KEY="$HOME/.ssh/id_ed25519"

export RUNPOD_PROXY_HOST="<pod-id>-8080.proxy.runpod.net"

export TARGET_REPO="owner/repo"

export READER_APP_ID="<reader_app_id>"
export READER_INSTALLATION_ID="<reader_installation_id>"
export READER_PEM="/absolute/path/to/reader.pem"

export PUBLISHER_APP_ID="<publisher_app_id>"
export PUBLISHER_INSTALLATION_ID="<publisher_installation_id>"
export PUBLISHER_PEM="/absolute/path/to/publisher.pem"
```

Define a helper:

```bash
vps_ssh() {
  ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -p "$VPS_PORT" "$VPS_USER@$VPS_HOST" -i "$VPS_KEY" "$@"
}
```

## 4. Install `computer-mcp`

Run the installer with an explicit HTTP listener port and public host hint:

```bash
vps_ssh "export COMPUTER_MCP_HTTP_BIND_PORT=8080 COMPUTER_MCP_PUBLIC_HOST=\"$RUNPOD_PROXY_HOST\"; curl -fsSL https://raw.githubusercontent.com/amxv/computer-mcp/main/scripts/install.sh | bash"
```

Why this is the preferred Runpod path:

- internal `8080` is plain HTTP for the Runpod proxy
- public HTTPS is terminated by Runpod on the proxy hostname
- ChatGPT sees a normal `https://...proxy.runpod.net/...` URL on standard `443`

## 5. Upload The GitHub App Keys

If `scp` is available, you can use it. The SSH pipe form below is more universal:

```bash
cat "$READER_PEM" | vps_ssh 'cat > /root/computer-mcp-reader.pem'
cat "$PUBLISHER_PEM" | vps_ssh 'cat > /root/computer-mcp-publisher.pem'
```

Install them into their final paths:

```bash
vps_ssh '
install -m 0600 -o computer-mcp-agent -g computer-mcp \
  /root/computer-mcp-reader.pem /etc/computer-mcp/reader/private-key.pem

install -m 0600 -o computer-mcp-publisher -g computer-mcp \
  /root/computer-mcp-publisher.pem /etc/computer-mcp/publisher/private-key.pem
'
```

## 6. Write The Minimal Config

Write the config explicitly so the Runpod HTTP listener is enabled:

```bash
vps_ssh "cat > /etc/computer-mcp/config.toml <<'EOF'
api_key = \"<existing_or_new_api_key>\"
http_bind_port = 8080
reader_app_id = ${READER_APP_ID}
reader_installation_id = ${READER_INSTALLATION_ID}
publisher_app_id = ${PUBLISHER_APP_ID}

[[publisher_targets]]
id = \"${TARGET_REPO}\"
repo = \"${TARGET_REPO}\"
default_base = \"main\"
installation_id = ${PUBLISHER_INSTALLATION_ID}
EOF
chgrp computer-mcp /etc/computer-mcp/config.toml
chmod 0640 /etc/computer-mcp/config.toml"
```

If you want to preserve the installer-generated API key, read it first and reuse it:

```bash
vps_ssh "sed -n 's/^api_key = \"\\(.*\\)\"$/\\1/p' /etc/computer-mcp/config.toml"
```

## 7. Start The Stack

```bash
vps_ssh 'computer-mcp start'
```

On Runpod this should start:

- `computer-mcp-prd` in process mode
- `computer-mcpd` in process mode
- internal HTTPS on `443`
- internal HTTP on `8080`

## 8. Verify Local Health On The Pod

```bash
vps_ssh 'computer-mcp status'
vps_ssh 'computer-mcp publisher status'
vps_ssh 'curl -fsS http://127.0.0.1:8080/health'
vps_ssh 'curl -kfsS https://127.0.0.1/health'
```

Expected local health result:

```json
{"status":"ok"}
```

## 9. Verify The Public Runpod URL

Check health from your local machine:

```bash
curl "https://${RUNPOD_PROXY_HOST}/health"
```

Print the full MCP URL shape:

```bash
vps_ssh "computer-mcp show-url --host \"$RUNPOD_PROXY_HOST\""
```

Expected shape:

```text
https://<pod-id>-8080.proxy.runpod.net/mcp?key=<api_key>
```

## 10. Verify A Real MCP Handshake

This is the fastest direct proof that the public URL is correct:

```bash
curl -sS -D - "https://${RUNPOD_PROXY_HOST}/mcp?key=<api_key>" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"curl","version":"0.1"}}}'
```

Expected result:

- `HTTP/2 200`
- `content-type: text/event-stream`
- a valid `initialize` response body

## 11. Fast Redeploy For Server-Only Releases

Once a Runpod pod is already working, do not rebuild the full Runpod image for every `computer-mcp` code change.

If the change is only in the Rust binaries, the preferred rollout is:

1. cut a new tagged release such as `v0.1.20`
2. wait for the GitHub `release` workflow to publish the Linux release artifact
3. SSH to the existing pod as `root`
4. run:

```bash
vps_ssh 'computer-mcp upgrade --version v0.1.20'
```

5. verify:

```bash
vps_ssh 'computer-mcp --version'
vps_ssh 'computer-mcp status'
curl "https://${RUNPOD_PROXY_HOST}/health"
curl -sS -D - "https://${RUNPOD_PROXY_HOST}/mcp?key=<api_key>" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"curl","version":"0.1"}}}'
```

This keeps the same pod and usually the same public Runpod proxy URL. Direct SSH port mappings may change if you later reset the pod, so rediscover those after any pod reset.

For this binary-only rollout path, you do not need to wait for `container-release`. The `computer-mcp upgrade --version vX.Y.Z` command now fetches `install.sh` from the same release tag, so the upgrade path is pinned to the version you released instead of whatever happens to be on `main`.

Only rebuild and redeploy the Runpod container image when the container environment changed. Examples:

- `Dockerfile.runpod`
- `docker/runpod-bootstrap.sh`
- `docker/runpod-run.sh`
- system package / toolchain changes
- SSH bootstrap or account setup changes
- template-level env, port, or storage changes

## Important Runpod Notes

1. Prefer the Runpod proxy hostname over the direct TCP URL for ChatGPT.
   Direct TCP usually gives a random external port, which is a worse fit for ChatGPT connectors.

2. The Runpod proxy hostname does public TLS for you.
   The internal `8080` listener is plain HTTP on purpose.

3. `computer-mcp` still keeps internal HTTPS on `443`.
   That direct listener is useful for debugging, but it is not the preferred public URL on Runpod.

4. If the pod is ephemeral and restarts without storage, rerun the install flow.
   Release artifacts keep that fast.

5. If the host is a normal VM with working `systemd`, do not use this guide.
   Use the main [README.md](../README.md) instead.
