# zodex Cloudflare Worker

This is the repository-owned source for the canonical Sprite front door. The
operator embeds this source at release build time and materializes a temporary
Wrangler project for `zodex sprite proxy deploy`; installed users do not need
this directory.

## Responsibilities

- expose non-secret Worker component/build status
- retry only idempotent Sprite health/readiness probes used for cold wake
- normalize `/mcp` to the Sprite origin's `/mcp/` path
- forward each incoming MCP request to the Sprite origin at most once
- preserve MCP query parameters and streamed responses
- disable caching for status, health, and MCP traffic

The Worker deliberately does not proxy documentation or retry a dispatched MCP
request after an upstream error/status. A tool call may already have executed
when an edge failure becomes visible, so replay would risk duplicate side
effects.

## Routes

- `/` and `/status` — Worker component/build metadata
- `/health` — retryable idempotent probe of `${SPRITE_ORIGIN}/health`
- `/mcp` — `${SPRITE_ORIGIN}/mcp/`
- `/mcp/` and descendants — corresponding Sprite MCP path
- all other paths — `404`

The checked-in Wrangler file is a template. Zodex fills the Worker name,
Sprite origin, and Worker build identity only in its temporary deploy project.
