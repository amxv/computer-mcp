---
title: "Local observability API"
description: "Build first-class read-only Local observability clients with the supported localhost HTTP API, Agent/workdir filters, invocation history, output pagination, and live SSE."
order: 1
category: Local Clients
summary: "The public versioned localhost HTTP/SSE contract behind `zodex local watch` and custom web, Swift, terminal, editor, and automation clients."
---

Zodex Local includes a **first-class observability API** for watching and inspecting ChatGPT tool activity without building on private Zodex internals. It is a normal versioned HTTP API on localhost, with a live SSE event stream and server-side filtering.

Use it to build whatever observability surface fits your workflow: a web dashboard, native Swift or menu-bar app, another terminal UI, an editor panel, a desktop app, or an automation/monitoring client in any language with HTTP + SSE support.

The first client of this API is the built-in [Local watch TUI](/docs/local-watch). `zodex local watch` uses the same discovery, Bearer auth, Agent resources, invocation endpoints, presentation model, filtered SSE stream, and gap-recovery path documented here. It does not use a privileged private backchannel.

Zodex Local intentionally separates **control** from **observation**. ChatGPT uses the authenticated Local MCP listener to run the three tools. Observability clients use a different, read-only, bearer-authenticated loopback HTTP server.

A dashboard should consume this public Local model. It should **not** parse `_meta["openai/session"]`, inspect the history SQLite schema directly, or depend on provider-specific opaque conversation keys.

## Client quick start

A Local observer client follows the same basic flow in any language:

1. Read the active runtime's `discovery.json`.
2. Read the observability bearer from the path published in discovery.
3. Check `schema_version`, `api_version`, `presentation_version`, and `runtime_id`.
4. Fetch `/v1/agents?runtime=current` and/or `/v1/invocations` for initial state.
5. Attach to `/v1/events`, optionally with `?agent_id=k7m2`, for live events from now onward.
6. On disconnect or an explicit `gap` event, recover from durable invocation/detail APIs and reconnect.

The API is intentionally sufficient to build a complete real-time interface without reading SQLite, parsing provider metadata, importing Rust types, or asking the MCP execution server for extra tools.

## Compatibility contract

Treat this as a public Local client API. Client implementations should depend on the documented versioned HTTP/SSE contract rather than private Rust modules or database tables.

When Zodex changes any of these, the API version/presentation version and this documentation must be reviewed together:

- routes or HTTP methods;
- response fields or field semantics;
- Agent/workdir filters;
- invocation query or pagination behavior;
- SSE event types or event payloads;
- gap/recovery semantics;
- discovery fields or credential-discovery behavior;
- `schema_version`, `api_version`, or `presentation_version`.

The goal is that a client written in Swift, TypeScript, Python, Rust, Go, or another language can stay on the supported API without following Zodex source refactors.

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
  "expires_at": "2026-08-17T08:00:00Z",
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

The HTTP listener exists only while Local is running. Durable history survives `zodex local stop`, but the observer API is not an always-on history service; after Local stops, use `zodex local history` until another Local runtime starts and publishes fresh discovery.

`expires_at` is the one **runtime-wide** TTL deadline. It belongs to the service, not to any Agent. A dashboard can display it directly from discovery without inventing per-Agent expiry.

`expires_at` is `null` when the runtime was started without a TTL.

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

Treat the bearer as a local secret. Zodex creates it with user-only file permissions and normally keeps it stable across runtime restarts so local clients can reconnect through discovery. If you intentionally rotate it:

```bash
zodex local setup --rotate-observability-bearer
```

clients should reread discovery and the bearer file rather than caching the old token.

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

`/observer/...` in that example is a route exposed by **your** trusted same-origin wrapper. The Zodex listener itself remains at the discovered `base_url`, with routes such as `$BASE/v1/events`.

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

### Errors and HTTP status

JSON failures use a small versioned envelope:

```json
{
  "schema_version": 1,
  "error": "human-readable message"
}
```

Expect:

- `400` for invalid filters or bounds such as a malformed Agent ID, relative workdir, unsupported `runtime` filter, `last=0`, or an output `limit` above the documented cap;
- `401` when the Bearer token is missing or wrong;
- `404` when a requested Agent/invocation does not exist;
- `500` when durable observer state cannot be read.

Every response, including errors, carries `Cache-Control: no-store`.

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

Response envelope:

```json
{
  "schema_version": 1,
  "api_version": 1,
  "presentation_version": 1,
  "runtime_id": "...",
  "history": {
    "database_exists": true,
    "physical_size_bytes": 123456,
    "health_state": "healthy",
    "health_reason": null,
    "over_budget": false,
    "last_retention_error": null
  },
  "current_runtime_agent_count": 2,
  "active_process_count": 1
}
```

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

List response envelope:

```json
{
  "schema_version": 1,
  "runtime_id": "...",
  "agents": [
    {
      "id": "k7m2",
      "first_seen_at_ms": 0,
      "last_seen_at_ms": 0,
      "seen_in_current_runtime": true,
      "active_process_count": 1,
      "workdirs": [
        {
          "normalized_workdir": "/Users/me/code/repo",
          "ordinal": 1,
          "first_seen_at_ms": 0,
          "last_seen_at_ms": 0,
          "first_invocation_id": 123,
          "last_invocation_id": 130,
          "retained_invocation_count": 8
        }
      ]
    }
  ]
}
```

`GET /v1/agents/{id}` uses the same Agent object under an `agent` field instead of `agents`.

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

`since_ms` is the ordinary history filter: it selects invocations whose **start** timestamp is at or after the supplied Unix-millisecond value. `recovery_since_ms` is for reconnect recovery: it includes invocations that started or completed in the window, logically in-flight invocations, and creators of still-active processes even when the original `exec_command` began before the recovery window. Use `recovery_since_ms` after an SSE disconnect/gap rather than trying to reconstruct that logic client-side.

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

The invocation-list envelope is:

```json
{
  "schema_version": 1,
  "presentation_version": 1,
  "runtime_id": "...",
  "invocations": [],
  "presentation": {
    "schema_version": 1,
    "agents": [],
    "records": []
  }
}
```

`invocations` is the logical evidence/query layer. `presentation` is the canonical normalized UI layer. A normal dashboard should render the presentation model and use logical records for detail/audit workflows rather than independently reimplementing Zodex's patch/poll/file-change interpretation.

## Canonical presentation model

The `presentation` object is the portable UI contract used by the first-party TUI. Its top-level shape is:

```json
{
  "schema_version": 1,
  "agents": [],
  "records": []
}
```

Each presentation record has common routing/evidence fields:

```json
{
  "raw_invocation_ids": [123],
  "agent_id": "k7m2",
  "declared_workdir": "/Users/me/code/repo",
  "normalized_workdir": "/Users/me/code/repo",
  "new_workdir": null,
  "started_at_ms": 0,
  "duration_ms": 1250,
  "evidence": {
    "evidence_state": "complete",
    "capture_state": "complete",
    "degraded": false,
    "reason": null
  },
  "kind": "command"
}
```

`kind` selects one normalized record shape:

| `kind` | Main fields | Intended UI meaning |
| --- | --- | --- |
| `command` | `command`, `status`, `effective_cwd`, `exit_code`, `termination_reason`, `output`, `output_truncated`, optional `polls` | One shell command, including current/final state and bounded display output. |
| `file_changes` | `source_tool`, `changes[]` | Structured created/edited/deleted/renamed files with line counts and optional diff lines. |
| `stdin` | `target_session_handle`, `chars`, `chars_truncated`, `creator_agent_id`, `cross_agent`, `result_status` | Explicit input sent to an existing process. |
| `kill` | `target_session_handle`, `creator_agent_id`, `cross_agent`, `result_status` | Explicit termination of an existing process. |
| `poll_aggregate` | `target_session_handle`, `count`, `final_status`, `creator_agent_id`, `caller_agent_ids`, `cross_agent` | Repeated no-input process polls folded into one compact UI record. |
| `generic` | `tool_name`, `status`, `summary` | Forward-compatible normalized fallback when a call does not fit a richer presentation kind. |

For `file_changes`, each change includes:

```json
{
  "operation": "edited",
  "path": "src/main.rs",
  "old_path": null,
  "write_mode": null,
  "added": 4,
  "removed": 2,
  "diff_truncated": false,
  "lines": [
    { "kind": "context", "old_line": 10, "new_line": 10, "text": "..." },
    { "kind": "remove", "old_line": 11, "new_line": null, "text": "..." },
    { "kind": "add", "old_line": null, "new_line": 11, "text": "..." }
  ]
}
```

`operation` is `created`, `edited`, `deleted`, or `renamed`. `write_mode`, when present, is `overwrite` or `append`.

`raw_invocation_ids` is intentionally an array because one normalized card can represent several logical MCP calls—for example folded polling. Use those IDs for raw/detail drill-down rather than assuming one presentation row always equals one invocation.

The presentation `agents` list carries each Agent's ID, first/last-seen timestamps, and normalized workdirs with first/last invocation IDs and retained counts. This lets a UI render an Agent/workdir picker without deriving identity from workdir strings.

## Invocation detail

```text
GET /v1/invocations/123
```

Detail contains one logical invocation, its canonical presentation, and output metadata:

```json
{
  "schema_version": 1,
  "presentation_version": 1,
  "runtime_id": "...",
  "invocation": {
    "id": 123,
    "agent_id": "k7m2",
    "tool_name": "exec_command"
  },
  "presentation": {
    "schema_version": 1,
    "agents": [],
    "records": []
  },
  "output": {
    "available": true,
    "chunk_count": 42,
    "size_bytes": 12345,
    "capture_state": "complete",
    "capture_reason": null,
    "first_cursor": 0
  }
}
```

The abbreviated `invocation` object above also contains the logical fields documented in the invocation-list section, including correlation, arguments, workdir, timing/outcome, result/error, evidence/capture state, and process/cross-Agent attribution when relevant.

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

The cursor is an output-chunk sequence cursor, not a page number. The server returns chunks with `sequence >= cursor`; when there is another page, `next_cursor` is the sequence of the first chunk not included in the current page. Pass it back unchanged on the next request.

Page envelope:

```json
{
  "schema_version": 1,
  "runtime_id": "...",
  "invocation_id": 123,
  "chunks": [
    {
      "sequence": 1,
      "observed_at_ms": 0,
      "text": "..."
    }
  ],
  "next_cursor": 2
}
```

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

Payloads are deliberately small notifications. Fetch canonical invocation/presentation state when a notification tells you something material changed:

| Event | `payload` |
| --- | --- |
| `agent_first_seen` | `{}` |
| `agent_workdir_added` | `{ "normalized_workdir": "..." }` |
| `invocation_started` | `{ "tool_name": "exec_command", "normalized_workdir": "/..." }` |
| `invocation_completed` | `{ "outcome": "success" }` or `{ "outcome": "error" }` |
| `presentation_updated` | `{}`; refetch invocation detail/presentation when the current rendered card matters |
| `output` | `{ "output_sequence": 12, "text": "..." }` |
| `output_complete` | `{}` |
| `process_started` | `{ "active_process_count": 2, "agent_active_process_count": 1 }` |
| `process_ended` | same count fields after decrement |
| `gap` | `{ "skipped_events": N, "recovery": "durable_history_or_invocation_detail" }` |

`agent_active_process_count` can be `null` for unattributed activity. `output` payload text is display-sanitized; the durable output endpoint is the evidence/drill-down surface.

SSE frames also set the SSE `id` field to the global runtime sequence and the SSE `event` field to the same value as JSON `event_type`. The server emits a keepalive approximately every 15 seconds. Do not treat keepalives as tool activity.

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
