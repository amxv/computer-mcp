# Computer Core + HTTP API + CLI Migration Spec

Status: draft architecture spec.

## 1. Objective

Refactor the repository so the three core remote-computer operations are transport-neutral while keeping the existing MCP surface intact and adding a first-party HTTP API plus a downloadable CLI binary named `computer`.

The intended end state is:

1. `computer-mcpd` still exposes `/mcp?key=...` for existing MCP clients.
2. The same daemon also exposes an HTTP API for the same three remote operations.
3. A new `computer` CLI talks to that HTTP API instead of embedding MCP client behavior.
4. Command/session semantics stay identical whether the caller is MCP, HTTP, direct Rust tests, or the CLI.

## 2. Product Boundary

The remote product surface for this migration is exactly these three operations:

- `exec_command`
- `write_stdin`
- `apply_patch`

`publish-pr` is intentionally outside that remote API boundary. It remains a local VPS capability implemented by the existing publisher daemon and is invoked through ordinary remote command execution when needed.

## 3. Current System Grounded In Code

The current `main` branch already contains most of the core behavior that the migration needs:

- `src/session.rs` is the authoritative implementation of PTY-backed process execution, session tracking, output truncation, idle timeout handling, process-group termination, live cwd reporting, and the running vs exited `ToolOutput` contract.
- `src/apply_patch.rs` is already transport-neutral. It rewrites relative patch paths against `workdir`, validates that workdir when needed, and delegates the actual patch application to `codex_apply_patch`.
- `src/protocol.rs` already defines transport-usable request models for `ExecCommandInput`, `WriteStdinInput`, and `ApplyPatchInput`, plus the shared `ToolOutput`, `CommandStatus`, and `TerminationReason` types.
- `src/server.rs` currently constructs the shared daemon state and directly invokes `SessionManager` or `apply_patch` from MCP tool handlers. It also builds the Axum router, serves `/health`, nests `/mcp`, and applies query-string API-key auth to the MCP route.
- `src/config.rs` already exposes one shared `api_key`, the normal TLS listener, and an optional `http_bind_port`, which means the daemon is already structured to serve the same Axum app on both HTTPS and optional plain HTTP.
- `src/publisher.rs` is already isolated behind a local Unix socket and should stay isolated. Nothing in the current remote transport design requires exposing it over HTTP.
- Packaging currently assumes three shipped binaries: `computer-mcp`, `computer-mcpd`, and `computer-mcp-prd`. That assumption exists in `Cargo.toml`, `.github/workflows/release.yml`, `scripts/install.sh`, `Dockerfile`, and `Dockerfile.runpod`.

## 4. Goals

- Extract a first-class service layer for `exec_command`, `write_stdin`, and `apply_patch`.
- Keep MCP behavior, tool names, schemas, annotations, auth shape, and URL shape unchanged.
- Expose the same operations through an HTTP API on the same daemon process.
- Ship a thin `computer` CLI that wraps the HTTP API.
- Preserve current session behavior, timeout behavior, cwd reporting, and patch behavior across all call paths.
- Keep the current publisher isolation model and local Unix-socket flow intact.

## 5. Non-Goals

- Do not replace MCP with HTTP. Both transports must coexist.
- Do not add a dedicated remote `publish-pr` API in the first pass.
- Do not redesign the publisher daemon architecture.
- Do not change the server-side operator workflow centered on `computer-mcp`.
- Do not introduce HTTP-only computer semantics that diverge from the current MCP behavior.

## 6. Required Architecture

The repository should be organized around one transport-neutral service boundary:

- the service owns computer semantics
- MCP adapts tool calls into the service
- HTTP adapts JSON requests into the service
- the `computer` CLI adapts terminal UX into HTTP requests

There must be exactly one authoritative implementation of:

- session allocation and lookup
- PTY-backed command execution
- output buffering and truncation
- idle timeout and termination behavior
- live cwd reporting
- patch application and `workdir` resolution

`computer-mcpd` remains the daemon entrypoint. The migration is an internal architecture change plus one new client surface, not a server replacement.

## 7. Module Responsibilities

### 7.1 `src/protocol.rs`

This remains the home for transport-neutral request and response types.

The existing request structs should remain the canonical input models:

- `ExecCommandInput`
- `WriteStdinInput`
- `ApplyPatchInput`

The existing `ToolOutput`, `CommandStatus`, and `TerminationReason` remain the canonical command/session result model. If HTTP-specific wrapper structs are needed for JSON ergonomics, they should be minimal and should not change the underlying semantics.

### 7.2 `src/service.rs`

Add a new service module as the center of the runtime.

Its responsibilities should be:

- own or reference the shared runtime state needed for the three core operations
- expose async methods for `exec_command`, `write_stdin`, and `apply_patch`
- hide the direct use of `SessionManager` and `apply_patch::apply_patch` from transport adapters
- remain directly usable from unit and integration tests without booting MCP or HTTP

The service must not know whether a request originated from MCP, HTTP, or a CLI.

### 7.3 `src/server.rs`

`src/server.rs` should become the daemon wiring and transport composition layer.

Its responsibilities should be:

- construct the shared service instance
- construct the Axum router
- preserve `/health`
- preserve `/mcp?key=...`
- add the new HTTP routes
- apply transport-specific auth middleware
- keep listener startup and shutdown behavior unchanged

It should no longer be the canonical home of computer-operation logic.

### 7.4 `src/client.rs`

Add a small shared HTTP client helper if it materially reduces duplication between CLI commands and future transport tests.

The helper should:

- own base URL and auth configuration
- serialize request structs from `src/protocol.rs`
- deserialize the shared response payloads
- centralize HTTP error handling

### 7.5 `src/bin/computer.rs`

Add a new binary named `computer`.

It should remain a thin client:

- no embedded MCP transport
- no VPS-only service-management commands
- no direct publisher access
- only the three remote computer operations plus convenience connection commands

## 8. Transport Contracts

### 8.1 MCP Contract

The MCP contract stays stable:

- route remains `/mcp`
- auth remains query-string `key=<api_key>`
- tool names remain `exec_command`, `write_stdin`, and `apply_patch`
- tool annotations remain unchanged
- request and response semantics remain unchanged

### 8.2 HTTP Contract

The HTTP API lives on the same daemon and the same Axum app as the MCP route.

The first-pass route set should be:

- `POST /v1/exec-command`
- `POST /v1/write-stdin`
- `POST /v1/apply-patch`

`GET /health` remains available as the daemon health endpoint.

HTTP requests should reuse the existing input structs from `src/protocol.rs`. HTTP responses must preserve the same semantic meaning currently exposed by the direct Rust call path and the MCP tool path, especially for:

- `status`
- `cwd`
- `session_id`
- `exit_code`
- `termination_reason`
- output text, including timeout and kill notices

### 8.3 Auth Contract

The daemon should continue to use the single configured `api_key`.

The transport-specific auth model should be:

- MCP: `?key=<api_key>`
- HTTP: `Authorization: Bearer <api_key>`

The HTTP API should not introduce a second secret or a second auth configuration path.

## 9. CLI Contract

The new binary is named `computer`.

It should support these remote operations directly:

- `computer exec-command ...`
- `computer write-stdin ...`
- `computer apply-patch ...`

It should also support convenience commands:

- `computer connect ...`
- `computer disconnect`

Connection resolution order must be:

1. explicit `--url` / `--key` flags
2. environment variables
3. saved connection profile from `connect`

The environment-variable interface should be:

- `COMPUTER_URL`
- `COMPUTER_KEY`

`connect` and `disconnect` are convenience only. The CLI must remain fully usable in stateless environments where no local profile exists.

The saved profile can remain intentionally simple in the first pass:

- a single current target
- user-scoped storage
- no multi-profile management requirement

## 10. Compatibility Invariants

The migration is only acceptable if these invariants remain true:

- the same logical request produces the same logical result regardless of whether it enters through MCP, HTTP, or direct service tests
- `src/session.rs` remains the source of truth for command/session behavior until any behavior is deliberately refactored into a new service module without semantic change
- `src/apply_patch.rs` remains the source of truth for patch path rewriting and application semantics until any behavior is deliberately refactored into a new service module without semantic change
- `publish-pr` remains local to the VPS and is not exposed as a new remote API
- the existing server install and upgrade flows continue to work after the new client binary is introduced

## 11. Packaging And Distribution Requirements

The project needs a downloadable `computer` client binary without regressing existing VPS install behavior.

That means the implementation must account for these current codebase realities:

- release packaging currently enumerates binaries explicitly
- `scripts/install.sh` currently locates a release archive by target triple and expects the server archive contents
- both Dockerfiles explicitly build and copy the current server-side binaries

The new client packaging must therefore be introduced in a way that does not break:

- tagged server installs via `scripts/install.sh`
- `computer-mcp upgrade`
- existing container image builds

## 12. Acceptance Criteria

This migration is complete when all of the following are true:

1. Existing MCP clients continue to work against `/mcp?key=...` without contract changes.
2. `computer-mcpd` also exposes an HTTP API for `exec_command`, `write_stdin`, and `apply_patch`.
3. MCP and HTTP both route into the same transport-neutral service layer.
4. A downloadable `computer` CLI can invoke the three core remote operations over HTTP using flags, environment variables, or an optional saved connection.
5. `publish-pr` remains a local VPS capability and is not exposed as a dedicated remote API.
6. Existing server release, install, and image flows remain functional after the client binary is added.
