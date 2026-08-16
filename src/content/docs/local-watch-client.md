---
title: "Local observer client (advanced)"
description: "Advanced reference for building a read-only Local dashboard or observer client from the supported localhost API and SSE stream."
order: 1
category: Local Clients
summary: "The versioned localhost observability contract used by `zodex local watch` and future dashboards."
---

This is an advanced client-author reference. If you only want to run or inspect Zodex Local, use [Local](/docs/local), [Local daily use](/docs/local-operations), or the built-in `zodex local watch`.
Zodex Local intentionally separates **control** from **observation**. ChatGPT uses the authenticated Local MCP listener to run the three tools. `zodex local watch` and custom dashboards use a different, read-only, bearer-authenticated loopback HTTP server.

A dashboard should consume this public Local model. It should **not** parse `_meta["openai/session"]`, inspect the history SQLite schema directly, or depend on provider-specific opaque conversation keys.

## Discovery first

While Local is running, runtime discovery is written to:

```text
${XDG_STATE_HOME:-~/.local/state}/zodex/local/runtime/discovery.json
```

Relevant fields are versioned and include:

```json
{
  "schema_version": 1,
  "runtime_id": "...",
  "pid": 12345,
  "start_directory": "/Users/me/code/repo",
  "started_at": "...",
  "expires_at": "... or null",
  "observability": {
    "api_version": 1,
    "presentation_version": 1,
    "base_url": "http://127.0.0.1:54321",
    "bearer_token_path": "/Users/me/.local/state/zodex/local/credentials/observability-bearer",
    "history_available": true,
    "sse_available": true
  }
}
```

Do not cache the base URL across runtime restarts. A new runtime can use a new random loopback port and a new `runtime_id`. Read discovery again when reconnecting.

`expires_at` is the one **runtime-wide** TTL deadline. It belongs to the service, not to any Agent. A dashboard can display it directly from discovery without inventing per-Agent expiry.

## Authentication and network boundary

Read the bearer from `observability.bearer_token_path`, then send:

```http
Authorization: Bearer <token>
```

Every observer response gets:

```http
Cache-Control: no-store
```

The listener binds only to IPv4 loopback. Missing/wrong Bearer auth returns `401`. It deliberately exposes no command/control route and no permissive arbitrary-origin CORS policy.

### Browser clients

Do **not** use unauthenticated `EventSource`: it cannot set the required `Authorization` header. Also do not ask Zodex to relax CORS just to make an arbitrary web origin convenient.

For a browser UI, use a deliberate same-origin localhost backend/wrapper (or a native/extension environment with explicit localhost access). Let that trusted local component read discovery + bearer and proxy an authorized fetch stream to its own same-origin frontend. If your environment can issue authorized same-origin `fetch`, SSE can be read as a streaming response:

```js
const response = await fetch("/observer/v1/events?agent_id=k7m2", {
  headers: { Authorization: `Bearer ${token}` }
});

if (!response.ok || !response.body) throw new Error("observer connection failed");
const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
for (;;) {
  const { value, done } = await reader.read();
  if (done) break;
  // Feed chunks into an SSE frame decoder; do not treat network chunk
  // boundaries as event boundaries.
  consumeSseBytes(value);
}
```

In an ordinary browser page on a different origin, the direct observer request is intentionally not CORS-enabled; route it through the trusted local wrapper instead.

## Resource map

The current observer API version is `1`:

```text
GET /v1/status
GET /v1/agents
GET /v1/agents/{id}
GET /v1/invocations
GET /v1/invocations/{id}
GET /v1/invocations/{id}/output
GET /v1/events
```

All routes are read-only.

## Status

```bash
curl -sS \
  -H "Authorization: Bearer $TOKEN" \
  "$BASE/v1/status"
```

The document includes:

- `schema_version`;
- `api_version`;
- `presentation_version`;
- `runtime_id`;
- durable history-store health;
- `current_runtime_agent_count`;
- runtime-wide `active_process_count`.

Use discovery, not `/v1/status`, for `start_directory`, `started_at`, and `expires_at`.

## Agents

List retained Agents:

```text
GET /v1/agents
```

Only Agents seen in the current runtime:

```text
GET /v1/agents?runtime=current
```

One Agent:

```text
GET /v1/agents/k7m2
```

Agent IDs match `[a-z0-9]{4}`. The same IDs are used by `watch`, history, API resources, and SSE filters.

An Agent resource includes first/last seen timestamps, whether it was seen in the current runtime, active-process count, and an ordered `workdirs` array. Each workdir record includes:

- `normalized_workdir`;
- first-seen ordinal;
- first/last timestamps;
- first/last invocation IDs;
- retained invocation count.

These are ordered unique **declared workdirs**. They are routing anchors, not confinement evidence. Never infer an Agent ID from a workdir; different Agents may use the same directory.

## Invocation list and filters

Recent normalized activity:

```text
GET /v1/invocations?last=50
```

`last` defaults to 50 and is capped at 100. Supported filters include:

```text
agent_id=k7m2
workdir=/absolute/repo/path
since_ms=<unix-ms>
recovery_since_ms=<unix-ms>
```

`workdir` must be absolute and is normalized using the same history semantics as the CLI.

The response contains both logical invocation records and one canonical `presentation` document. Prefer the presentation model for normal UI. It is versioned by `presentation_version` and already applies the Local normalization rules for commands, file changes, poll folding, Agent/workdir context, and degraded evidence.

Logical records retain fields useful for drill-down, including:

- durable `id` and `correlation_id`;
- `agent_id` (or `null` for unattributed calls);
- `tool_name` and logical arguments;
- exact and normalized declared workdir;
- `is_new_workdir`;
- start/completion/duration/outcome;
- evidence/capture state and reason;
- target session handle/creator Agent where applicable;
- `cross_agent` for process continuation attribution.

Do not turn provider metadata into a second identity model. The API has already converted correlation into the public short Agent identity.

## Invocation detail

```text
GET /v1/invocations/123
```

Detail contains one logical invocation, its canonical presentation, and output metadata:

```json
{
  "available": true,
  "chunk_count": 42,
  "size_bytes": 12345,
  "capture_state": "complete",
  "capture_reason": null,
  "first_cursor": 0
}
```

Treat `evidence_state` / `capture_state` as user-visible truth. Do not present incomplete evidence as complete just because a tool result exists.

## Full output pagination

The output returned to ChatGPT is deliberately bounded. Durable Local history can hold the fuller PTY stream separately.

Read it in pages:

```text
GET /v1/invocations/123/output?cursor=0&limit=16
```

`limit` defaults to 16 and is capped at 64. Each chunk contains:

```json
{
  "sequence": 1,
  "observed_at_ms": 0,
  "text": "..."
}
```

Continue with `next_cursor` until it becomes `null`.

Full output and exact logical records are evidence, not automatically safe terminal markup. Render raw text inertly and sanitize control sequences at your display boundary. The canonical presentation is the normal UI authority; raw evidence is explicit drill-down.

## Live SSE

Attach from **now**:

```bash
curl -N \
  -H "Authorization: Bearer $TOKEN" \
  "$BASE/v1/events"
```

Filter server-side to one Agent:

```bash
curl -N \
  -H "Authorization: Bearer $TOKEN" \
  "$BASE/v1/events?agent_id=k7m2"
```

The stream does not replay old history on connect. Events are runtime-scoped and carry:

```json
{
  "schema_version": 1,
  "runtime_id": "...",
  "sequence": 17,
  "emitted_at_ms": 0,
  "event_type": "invocation_started",
  "agent_id": "k7m2",
  "invocation_id": 123,
  "presentation_revision": 1,
  "payload": {}
}
```

Current event types include:

```text
agent_first_seen
agent_workdir_added
invocation_started
invocation_completed
presentation_updated
output
output_complete
process_started
process_ended
gap
```

`output` live payloads are display-sanitized. `agent_workdir_added` carries the normalized workdir. Process events carry runtime-wide and Agent-local active process counts.

### Sequence semantics

The sequence is global to one runtime. If you use an Agent-filtered stream, gaps in visible numeric sequence are normal because unrelated Agents are omitted. Do not treat every numeric jump as data loss.

Only an explicit SSE event with `event_type: "gap"` signals broadcast lag that requires durable recovery. Its payload includes `skipped_events` and:

```json
{
  "recovery": "durable_history_or_invocation_detail"
}
```

## Reconnect and gap recovery

Keep a local timestamp/sequence watermark for UI bookkeeping, but use durable API state as authority after disconnect or `gap`:

1. reread `discovery.json`; if `runtime_id` changed, treat it as a new runtime and reset runtime-scoped sequence state;
2. refresh `/v1/status` and `/v1/agents` (or one Agent detail);
3. query `/v1/invocations?recovery_since_ms=<last-known-ms>` with the active Agent/workdir filters;
4. fetch detail for any invocation whose current presentation/output state matters;
5. reconnect `/v1/events` and continue from now.

The recovery query can include active process creators even when their initial invocation predates the reconnect window, so a running command does not disappear merely because the viewer joined late.

## Minimal Python client

The repository includes `examples/local_observability_client.py`. It reads the discovery document and bearer itself, lists Agents, then optionally attaches to the authorized SSE stream. It uses only the public JSON/SSE contract and never reads provider metadata or SQLite tables.

List Agents:

```bash
python3 examples/local_observability_client.py
```

Watch one Agent:

```bash
python3 examples/local_observability_client.py --agent k7m2 --events
```

That example is intentionally small. Production clients should add retry/backoff, bounded UI state, explicit runtime-change handling, durable gap recovery, terminal-safe rendering, and API/presentation version checks before consuming newer schemas.

## Version handling

At minimum, check:

- discovery `schema_version`;
- observer `api_version`;
- `presentation_version`;
- live event `schema_version`;
- `runtime_id` on every runtime-scoped resource/event.

Fail clearly when a version is newer than the client understands. Do not guess field meanings from provider payloads or SQLite layout to bypass versioning.
