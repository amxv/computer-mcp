---
title: "Connect ChatGPT"
description: "Use the canonical Cloudflare Worker front door, understand the secret capability URL, and add a Sprite deployment to ChatGPT."
order: 2
category: Sprite
summary: "Verify the raw wake origin and permanent Worker, then use zodex sprite connect to copy the ChatGPT MCP endpoint safely."
---

This page is **Sprite-specific**. Zodex Local connects through the [OpenAI Secure MCP Tunnel](/docs/local/setup) and does not use this Cloudflare/Sprite front door.

## The supported path

```text
ChatGPT
  → https://<worker>.workers.dev/mcp?key=<secret>
  → Cloudflare Worker
  → public https://<sprite>.sprites.app wake edge
  → zodexd HTTP service
```

The Worker is canonical because it can perform idempotent readiness/wake checks before forwarding a request. Once an MCP request is dispatched upstream, Zodex forwards it **at most once**; it does not replay a possibly side-effecting tool call after an ambiguous failure.

## Raw origin versus capability URL

The Sprite's raw HTTPS URL must be public so Cloudflare can wake and reach it. Inspect it with current Sprite CLI syntax:

```bash
sprite info dev
```

If URL auth needs repair:

```bash
sprite config update --url-auth public dev
```

A public raw origin is **not** equivalent to public Zodex execution. `/mcp` still requires the Zodex query key, and the deleted privileged `/v1` command API is not part of the public surface.

## Verify each layer

```bash
zodex sprite proxy status --sprite dev
zodex sprite proxy verify --sprite dev
zodex sprite health --sprite dev
```

`proxy verify` checks both the raw origin and the registered Worker. `health` checks the supported end-to-end Sprite chain.

If no permanent Worker is registered, deploy it from the installed operator:

```bash
zodex sprite proxy deploy --sprite dev
```

No source checkout or hand-edited Wrangler project is required.

### First unauthenticated Cloudflare deploy

With a current Wrangler-capable runner, Zodex can fall back to Wrangler's temporary deployment flow. It prints a live Worker URL and one-time claim URL.

- Treat the **claim URL as a secret**.
- Claim it within **60 minutes**.
- Zodex does not persist it.
- Wrangler may cache temporary deployment credentials/claim state in its own global config directory.
- Claiming the Worker does not give future CLI sessions permanent Cloudflare authentication.

After claim:

```bash
wrangler login --use-keyring
zodex sprite proxy deploy --sprite dev
```

If several Cloudflare accounts are available, Zodex refuses to guess. Pass the intended account:

```bash
zodex sprite proxy deploy --sprite dev --cloudflare-account <id-or-name>
```

## Copy the ChatGPT endpoint

```bash
zodex sprite connect --sprite dev
```

`connect` requires a current registered Worker, validates that it points to the selected Sprite origin, reads the remote MCP key, and tries to copy the full endpoint to the clipboard.

Use this only when you explicitly want the URL printed:

```bash
zodex sprite connect --sprite dev --show-url
```

The URL is HTTPS, so it is encrypted in transit. The security concern is **credential-in-URL leakage**: shell history, logs, screenshots, pasted chats, and browser history can expose the full capability. Treat the entire endpoint as a secret.

## Add the custom app in ChatGPT

OpenAI's current guidance for custom MCP apps on ChatGPT web is:

1. enable Developer Mode in **Settings → Apps** (or Workspace Settings → Apps);
2. create a custom app and provide the MCP endpoint;
3. scan the tools;
4. create the app.

Zodex's endpoint already embeds its capability secret. Do not paste that key into an unrelated authentication field. OpenAI's current custom-app guide documents OAuth where authentication is used, but does not document a static custom-header/API-key field for this flow.

Most OpenAI paid plans support custom MCP servers.

## Reconnect later

You do not need to reconstruct the endpoint from runtime config. Run:

```bash
zodex sprite connect --sprite dev
```

If it says the Worker is stale or foreign, repair the front door first:

```bash
zodex sprite proxy status --sprite dev
zodex sprite proxy deploy --sprite dev
zodex sprite proxy verify --sprite dev
```
