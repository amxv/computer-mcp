---
title: "Sprite: connect ChatGPT"
description: "Connect ChatGPT to a Sprite-hosted Zodex MCP server, verify the public origin, and use the optional Cloudflare front door when needed."
order: 8
category: Operations
summary: "The Sprite MCP URL, API key, origin checks, ChatGPT app setup, and optional proxy deployment."
---

This page is **Sprite-specific**.

Zodex Local does not require this Cloudflare/Sprite proxy. Use [Local setup and ChatGPT connection](/docs/local-setup) for the Mac path.

## The URL ChatGPT needs

A Sprite deployment exposes Zodex over HTTPS. The MCP URL has this shape:

```text
https://<sprite-host>/mcp?key=<zodex-api-key>
```

Treat the complete URL as a secret because the query parameter is the Zodex MCP credential.

## Get the Sprite host

```bash
sprite use dev
sprite url
```

If you are handing ChatGPT the raw Sprite URL, first verify that Zodex is healthy there:

```bash
zodex sprite health --sprite dev
zodex proxy verify-origin --sprite dev
```

A healthy origin should reach the Zodex health/MCP routes consistently.

## Add it to ChatGPT

In ChatGPT, create a custom MCP app/connector and enter the full `/mcp?key=...` URL.

Because the Zodex key is already part of the URL, choose **No authentication** at the ChatGPT connector layer unless your own front door adds a separate authentication scheme.

Scan the MCP tools. You should see exactly:

```text
exec_command
write_stdin
apply_patch
```

Then open a fresh chat, select/enable the app, and try a harmless command against an explicit Sprite workdir.

## When to use the Cloudflare proxy

Some deployments use the repository's Cloudflare Worker as a stable public front door instead of handing ChatGPT the raw Sprite URL.

Use it when the raw Sprite origin has cold-start, routing, or path-normalization behavior that makes direct MCP unreliable in your environment.

Do **not** deploy the proxy merely because it exists. Verify the raw origin first:

```bash
zodex proxy inspect --sprite dev
zodex proxy verify-origin --sprite dev
```

If the raw origin works reliably for your client, you may not need the extra layer.

## Deploy the repository Cloudflare Worker

From a repository checkout:

```bash
cd proxy/cloudflare-worker
```

Set `vars.SPRITE_ORIGIN` in `wrangler.jsonc` to the Sprite origin, then:

```bash
npx wrangler deploy
```

Or use the operator helper where appropriate:

```bash
zodex proxy deploy --sprite dev
```

`update` is an alias for the deploy path.

## Verify after deployment

Test the proxy's health endpoint and MCP connection separately from the raw Sprite origin.

If the proxy fails but the Sprite origin works, debug the Worker/configuration—not the Zodex guest runtime.

If both fail, start with [Sprite troubleshooting](/docs/sprite-troubleshooting).
