# Computer Core + HTTP API + CLI Migration Spec

Status: draft migration plan for implementation.

## 1. Objective

Refactor the repository so the actual remote-computer capabilities live in a transport-neutral core, while keeping the existing MCP deployment path intact and adding a new plain HTTP API plus a downloadable terminal client named `computer`.

The desired end state is:

1. `computer-mcpd` continues to run on the VPS and continues to expose `/mcp?key=...` for MCP clients.
2. The same running daemon also exposes a stable HTTP API for non-MCP clients.
3. A new binary named `computer` can be downloaded into another Linux container and can invoke the remote machine capabilities without the caller having to think about raw HTTP details.
4. The core execution semantics remain identical regardless of whether the caller is an MCP client, the HTTP API, direct Rust tests, or the future CLI.

## 2. Why This Refactor Is Worth Doing

The current repository already contains the real computer-control primitives. The important observation is that those primitives are more valuable than MCP itself:

- `exec_command` provides remote process execution with PTY behavior and output capture.
- `write_stdin` provides stateful session continuation and remote process interaction.
- `apply_patch` provides structured file editing without exposing an unrestricted raw write API.

Those three capabilities are the actual product. MCP is only one packaging mechanism for them.

The refactor should therefore make the codebase reflect the product truth:

- the remote-computer core is the center of the architecture
- MCP is one adapter
- HTTP is another adapter
- the `computer` terminal client is another adapter or client layer

## 3. Current State Summary

The current code already has a useful partial separation.

### 3.1 What Is Already Well Isolated

- `src/session.rs` contains the process and session engine.
- `src/apply_patch.rs` wraps `codex-apply-patch` and resolves paths relative to `workdir`.
- `src/publisher.rs` contains the local PR publishing flow.
- `src/protocol.rs` already defines request and response structs that can be reused.

### 3.2 What Is Still Too Coupled To MCP

`src/server.rs` currently owns the shared state and calls `SessionManager` directly from MCP tool handlers. That means the service boundary still conceptually lives inside the MCP adapter.

As a result:

- MCP is currently the first-class transport
- there is no transport-neutral service layer
- adding a plain HTTP API would currently tempt duplication or awkward cross-calls

### 3.3 Deployment Shape That Must Remain Supported

The deployed VPS and Runpod shape must continue to work:

- `computer-mcpd` remains the server daemon
- `computer-mcp-prd` remains the local publisher daemon
- `computer-mcp` remains the operator/admin CLI on the VPS
- Runpod deployment continues to expose the public proxy URL and automatic bootstrap flow

## 4. Goals And Non-Goals

### 4.1 Goals

- extract a first-class reusable execution service from the current MCP server wiring
- keep MCP behavior and endpoint compatibility unchanged
- add a plain HTTP API that maps closely to the same request and response model
- add a new `computer` CLI binary that wraps the HTTP API
- preserve existing session semantics, timeout behavior, and patch behavior across all surfaces
- preserve the current `publish-pr` security model by continuing to execute it locally on the VPS

### 4.2 Non-Goals

- do not redesign the publisher daemon architecture in this migration
- do not add a dedicated remote `publish-pr` protocol surface in the first pass
- do not replace MCP with HTTP; both surfaces should exist together
- do not change the current Runpod operational model beyond building and shipping the updated binaries

## 5. Architectural End State

The repository should be organized around one transport-neutral service layer.

The core mental model should be:

- the service owns computer semantics
- MCP maps tool invocations into the service
- HTTP maps JSON requests into the service
- the `computer` client maps terminal UX into HTTP requests

There should be exactly one authoritative implementation of:

- session allocation and lookup
- PTY-backed command execution
- output buffering and truncation
- idle timeout and termination behavior
- patch application and `workdir` resolution

If an MCP caller and an HTTP caller send the same logical request, they should receive the same logical result.

## 6. Proposed Module Layout

The recommended target layout is:

- `src/protocol.rs`
- `src/session.rs`
- `src/apply_patch.rs`
- `src/publisher.rs`
- `src/service.rs` new
- `src/server.rs` reduced to MCP adapter responsibilities
- `src/http_api.rs` new
- `src/client.rs` optional shared HTTP client helper for the new CLI

### 6.1 `src/protocol.rs`

This file should remain the home of transport-neutral request and response structs.

Existing types can largely remain in place:

- `ExecCommandInput`
- `WriteStdinInput`
- `ApplyPatchInput`
- `CommandStatus`
- `TerminationReason`
- `ToolOutput`

An optional cleanup would be to rename `ToolOutput` to something transport-neutral such as `CommandOutput`, but that rename is not required for the migration to succeed.

### 6.2 `src/service.rs`

This new module should become the center of the runtime.

Suggested responsibilities:

- owns the shared runtime state used by computer operations
- owns or references the `SessionManager`
- exposes async methods for `exec_command`, `write_stdin`, and `apply_patch`
- is directly usable from tests without MCP or HTTP involved

The service should not know whether a request originated from MCP, HTTP, or a test.

### 6.3 `src/server.rs`

`src/server.rs` should become an MCP adapter only.

It should:

- instantiate the shared service object
- translate MCP tool invocations into service method calls
- preserve the existing tool names and MCP annotations
- preserve the existing `/mcp?key=...` behavior

It should not contain the canonical business logic for command execution or patch application.

### 6.4 `src/http_api.rs`

This new module should expose a plain JSON API backed by the same service object.

Its purpose is not to invent new semantics. Its purpose is to expose the same semantics through a simpler client surface than MCP.

### 6.5 `src/client.rs`

If useful, add a small shared HTTP client helper that can be reused by the `computer` CLI binary and future internal tests.

The client helper should:

- own base URL and auth configuration
- serialize request structs from `protocol.rs`
- deserialize shared response structs from `protocol.rs`
- centralize HTTP error handling and retry-free request execution

## 7. HTTP API Contract

The HTTP API should live beside the MCP endpoint on the same daemon.

Suggested routes:

- `GET /health`
- `GET /v1/handshake`
- `POST /v1/exec-command`
- `POST /v1/write-stdin`
- `POST /v1/apply-patch`

The service should continue to expose `/mcp` exactly as it does today.

### 7.1 Auth Model

Do not change the existing MCP query-string auth model.

- MCP remains `GET/POST /mcp?key=<api_key>` according to current behavior.

For the new HTTP API, use header-based auth instead of query-string auth.

Recommended shape:

- `Authorization: Bearer <api_key>`

Reasons:

- cleaner CLI ergonomics
- fewer accidental secrets in shell history and logs
- no impact on existing MCP compatibility

### 7.2 Payload Model

The HTTP API should reuse the same Rust structs already used by the server logic.

- `POST /v1/exec-command` accepts `ExecCommandInput` and returns the same output shape used today.
- `POST /v1/write-stdin` accepts `WriteStdinInput` and returns the same output shape used today.
- `POST /v1/apply-patch` accepts `ApplyPatchInput` and returns the patch result string.

This is important because the HTTP surface should be a transport change, not a semantic fork.

## 8. `computer` CLI Contract

The new binary should be named `computer`.

It should be designed as a thin terminal wrapper over the HTTP API, not as an embedded MCP client.

Recommended commands:

- `computer connect <url>`
- `computer exec-command ...`
- `computer write-stdin ...`
- `computer apply-patch ...`
- `computer disconnect`

`connect` should save connection metadata locally, such as base URL, API key, and optional TLS settings.

`disconnect` should remove or disable that stored profile.

The CLI should also support stateless execution for model environments that do not want to persist config:

- `computer --url <url> --key <key> exec-command ...`

### 8.1 `publish-pr`

Do not add a dedicated remote HTTP `publish-pr` endpoint in this migration.

The intended model is:

- keep `publish-pr` local to the VPS
- run it through remote `exec_command` when needed
- preserve the current publisher-daemon and Unix-socket flow on the VPS

That means the three remote computer primitives remain the true API surface, and `publish-pr` remains an ordinary command executed on the target host.

## 9. Migration Sequence

The implementation should happen in ordered phases so behavior stays stable while the refactor lands.

### Phase 1: Introduce The Shared Service Layer

- add `src/service.rs`
- move the shared runtime state and the `SessionManager` ownership there
- expose service methods that accept the existing protocol structs
- keep behavior identical to current MCP behavior

This phase should not yet change the public HTTP or MCP surface.

### Phase 2: Make MCP Call The Shared Service

- update `src/server.rs` so MCP handlers call the new service object
- keep existing tool names and annotations unchanged
- keep `/mcp?key=...` unchanged

At the end of this phase, the MCP adapter should be thin.

### Phase 3: Add The HTTP API

- add `src/http_api.rs`
- mount the HTTP routes on the existing Axum app beside `/mcp`
- reuse the same service object and the same protocol structs
- add header-based auth middleware for the new routes

At the end of this phase, the daemon should serve both transports from the same process.

### Phase 4: Add The `computer` Client Binary

- add a new binary target for `computer`
- add CLI parsing for connect, disconnect, exec-command, write-stdin, and apply-patch
- use the HTTP API rather than embedding MCP client behavior
- optionally add a shared `src/client.rs` helper for request execution

At the end of this phase, a separate Linux container should be able to download `computer` and operate the VPS over HTTP.

### Phase 5: Package And Ship

- include the new binary in release artifacts
- update any installer or release workflows that enumerate shipped binaries
- update `Dockerfile.runpod` so the Runpod image includes the updated daemon and the new `computer` binary
- update docs to mention both `/mcp` and `/v1/...`

## 10. Test Plan

### 10.1 Service-Layer Tests

Add tests that hit the service layer directly with no transport involved.

These tests should verify:

- `exec_command` returns running vs exited states correctly
- `write_stdin` continues sessions correctly
- idle timeout semantics remain unchanged
- output truncation remains unchanged
- `apply_patch` behavior remains unchanged for relative and absolute paths

### 10.2 MCP Compatibility Tests

Existing MCP-facing tests should continue to pass with minimal changes.

Add or preserve tests proving:

- the three tool names remain present
- handler schemas remain correct
- existing MCP clients still see the same tool contract

### 10.3 HTTP API Tests

Add HTTP integration tests proving:

- auth succeeds with the configured bearer token
- auth fails without or with incorrect credentials
- `POST /v1/exec-command` returns the expected running or exited shape
- `POST /v1/write-stdin` continues an existing session correctly
- `POST /v1/apply-patch` behaves identically to the service and MCP paths

### 10.4 CLI Tests

Add focused tests for the `computer` binary where practical.

The highest-value checks are:

- config persistence for `connect` and `disconnect`
- stateless invocation with `--url` and `--key`
- human-readable error messaging when the server is unreachable or unauthorized

## 11. Deployment And Runpod Implications

This refactor fits the current Runpod model well because the pod already exposes a stable public HTTP proxy hostname and already starts the daemon automatically.

Operational implications should be kept intentionally small:

- the daemon continues to bind the existing MCP endpoint
- the same daemon also serves `/v1/...`
- Runpod bootstrap remains automatic
- image updates remain the same basic process they are today

In other words, this should be a code architecture change, not an operator workflow rewrite.

## 12. Acceptance Criteria

This migration is complete when all of the following are true:

1. `computer-mcpd` still exposes the existing MCP endpoint and current MCP clients continue to work.
2. The daemon also exposes a documented HTTP API for the three core computer primitives.
3. The HTTP API and MCP surface both route through the same shared service layer.
4. The `computer` CLI can connect to a running VPS instance and invoke `exec-command`, `write-stdin`, and `apply-patch` without the caller manually crafting HTTP requests.
5. `publish-pr` remains executable on the VPS through the normal command path rather than requiring a new dedicated remote API.
6. Runpod deployment still works through the existing image and bootstrap model with only minimal packaging updates.

## 13. Implementation Notes For The Coding Pass

- start by extracting behavior, not renaming everything at once
- preserve the existing protocol structs wherever practical to reduce migration risk
- keep the MCP adapter intentionally thin
- avoid inventing HTTP-only semantics
- land the refactor in phases so MCP compatibility can be checked after each stage

The safest way to implement this is to make the internals more modular first, then add the new surfaces second.
