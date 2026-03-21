# Concurrent Session Broker Plan

## State of Current System

`computer-mcpd` exposes one MCP endpoint and one `/v1/...` HTTP API, both backed by a single shared `ComputerService`.

Today:

- `exec_command` and `write_stdin` both acquire the same `Arc<Mutex<SessionManager>>` in [`src/service.rs`](/Users/ashray/code/amxv/computer-mcp/src/service.rs).
- The session-manager lock stays held while the request waits for process output or yield expiry in [`src/session.rs`](/Users/ashray/code/amxv/computer-mcp/src/session.rs).
- Multiple sessions can exist in memory at once, but shell operations across different sessions are effectively serialized by that global lock.
- Session continuation is authorized only by a numeric `session_id`, so any caller that learns the id can continue the shell.
- The protocol and client assume `write_stdin` targets a numeric `session_id`.
- `apply_patch` is already stateless relative to the session manager and does not participate in the global lock.

Observed live behavior on the `computer` Sprite matches the code:

- one `exec_command` call blocks a second concurrent `exec_command` call until the first returns
- `apply_patch` can still complete while a different `exec_command` is in flight

The user-approved direction for this feature is:

- keep one fixed `computer-mcpd` server and one fixed MCP URL
- support multiple concurrent agents and multiple concurrent shell sessions behind that one server
- ensure only the creator/capability-holder of a shell can continue it with `write_stdin`
- add robust server-side session metadata for auditing and debugging
- do not add hard workspace boundaries or OS-level isolation as part of this change

## State of Ideal System

The daemon behaves like a concurrent session broker:

- one public MCP endpoint and one `/v1/...` API remain unchanged
- each `exec_command` creates an independently managed shell session
- different sessions can run, wait, and stream output concurrently without blocking each other
- only the holder of an opaque server-issued session capability can continue or poll that session
- session metadata is available for logging, diagnostics, and future admin tooling
- per-session operations serialize only within that session, never across the whole daemon
- existing timeout, PTY, output buffering, live cwd reporting, and process-group termination behavior remain intact

## Plan Phases

### Phase 1: Define The New Session Contract

#### Files to read before starting

- [`src/protocol.rs`](/Users/ashray/code/amxv/computer-mcp/src/protocol.rs)
- [`src/service.rs`](/Users/ashray/code/amxv/computer-mcp/src/service.rs)
- [`src/session.rs`](/Users/ashray/code/amxv/computer-mcp/src/session.rs)
- [`src/http_api.rs`](/Users/ashray/code/amxv/computer-mcp/src/http_api.rs)
- [`src/server.rs`](/Users/ashray/code/amxv/computer-mcp/src/server.rs)
- [`src/client.rs`](/Users/ashray/code/amxv/computer-mcp/src/client.rs)
- [`src/bin/computer.rs`](/Users/ashray/code/amxv/computer-mcp/src/bin/computer.rs)

#### What to do

Define and implement the protocol shape for session ownership and metadata without changing the public endpoint layout.

Make these decisions explicit in code:

- running commands return an opaque `session_handle` string
- `write_stdin` requires `session_handle`
- numeric `session_id` may remain as optional diagnostic output during migration, but it must stop being the authorization key
- session records store metadata including:
  - server-generated internal id
  - opaque handle
  - created time
  - last-used time
  - initial command
  - current cwd / last known cwd
  - transport kind (`mcp` or `http`)
  - optional caller label if it can be captured cheaply without complicating the tool interface

Protocol recommendation:

- add `session_handle: Option<String>` to `ToolOutput`
- add `session_handle: String` to `WriteStdinInput`
- keep `session_id` only if needed for backward-compatible display/tests while the client and server migrate together in this repo

Implementation guidance:

- keep the tool surface minimal
- do not add workspace-boundary enforcement in this phase
- do not add multiple daemon instances or new public routes

#### Validation strategy

- unit tests for protocol serialization/deserialization
- update existing tests that assume `session_id` is the continuation key
- add tests proving `write_stdin` rejects missing or unknown `session_handle`

#### Risks / fallbacks

- Risk: partially migrating the protocol can leave server and CLI out of sync
- Fallback: temporarily return both `session_id` and `session_handle`, but require `session_handle` for continuation inside this repo’s client/tests

### Phase 2: Replace The Global Session Lock With Concurrent Per-Session Runtimes

#### Files to read before starting

- [`src/service.rs`](/Users/ashray/code/amxv/computer-mcp/src/service.rs)
- [`src/session.rs`](/Users/ashray/code/amxv/computer-mcp/src/session.rs)
- [`src/config.rs`](/Users/ashray/code/amxv/computer-mcp/src/config.rs)

#### What to do

Refactor the runtime so different sessions do not block each other.

Target architecture:

- `ComputerService` owns a lightweight concurrent session registry instead of `Arc<Mutex<SessionManager>>`
- the registry is responsible only for:
  - inserting new sessions
  - resolving a handle to a session entry
  - removing dead sessions
  - enforcing `max_sessions`
- each live session is represented by its own runtime object / actor that owns:
  - child process
  - PTY writer
  - output buffer
  - timeout state
  - kill state
  - last-known cwd
  - per-session operation serialization

Operational rules:

- `exec_command` creates a session runtime and waits only on that session’s yield/exit
- `write_stdin` routes only to the targeted session runtime
- two unrelated sessions must be able to wait concurrently
- two overlapping operations on the same session must still serialize correctly

Implementation notes:

- do not hold any global lock while waiting for yield/exit
- preserve current PTY-backed shell behavior
- preserve process-group termination logic
- preserve live cwd reporting from `/proc`
- preserve output truncation behavior
- preserve timeout and `kill_process` semantics

Design preference:

- a per-session actor model is preferred over finer-grained shared locking because PTY writes, output draining, and timeout handling are session-local and become easier to reason about

#### Validation strategy

- add a concurrency regression test proving two overlapping `exec_command` calls on different sessions complete independently and the fast one is no longer delayed by the slow one
- add a regression test proving `write_stdin` on one session does not block `exec_command` on another session
- add a regression test proving same-session operations still serialize correctly
- keep existing timeout / kill / cwd / env persistence tests green

#### Risks / fallbacks

- Risk: introducing races around process exit, output draining, or PTY writer reuse
- Fallback: keep a very small per-session mutex/actor mailbox if needed, but never reintroduce a daemon-wide wait lock

### Phase 3: Plumb Session Ownership Through MCP, HTTP, And The CLI

#### Files to read before starting

- [`src/server.rs`](/Users/ashray/code/amxv/computer-mcp/src/server.rs)
- [`src/http_api.rs`](/Users/ashray/code/amxv/computer-mcp/src/http_api.rs)
- [`src/client.rs`](/Users/ashray/code/amxv/computer-mcp/src/client.rs)
- [`src/bin/computer.rs`](/Users/ashray/code/amxv/computer-mcp/src/bin/computer.rs)
- [`README.md`](/Users/ashray/code/amxv/computer-mcp/README.md)
- [`gg/agent-outputs/computer-cli-quickstart-for-agents.md`](/Users/ashray/code/amxv/computer-mcp/gg/agent-outputs/computer-cli-quickstart-for-agents.md)

#### What to do

Update the transports and local client so the new session contract is used end to end.

Required outcomes:

- MCP tool handlers return the new `session_handle` on running sessions
- `/v1/write-stdin` accepts `session_handle`
- the Rust client uses `session_handle` for verification and continuation
- the `computer` CLI prints/consumes the new session handle shape cleanly
- any internal probing logic that currently fabricates a numeric session id is updated

Metadata work for this phase:

- attach transport-origin metadata when creating sessions
- if cheap and low-risk, attach a caller label in the HTTP API and client paths
- do not expand the tool surface unnecessarily for model-facing usage unless there is a clear need

#### Validation strategy

- HTTP API parity tests remain green
- MCP parity tests remain green
- CLI/client tests are updated for `session_handle`
- connection verification still works against the live API contract

#### Risks / fallbacks

- Risk: CLI or docs drift from the server contract
- Fallback: keep compatibility output temporarily, but do not leave numeric-id continuation as the real authorization path

### Phase 4: Add Observability, Session Lifecycle Tests, And Documentation

#### Files to read before starting

- [`src/session.rs`](/Users/ashray/code/amxv/computer-mcp/src/session.rs)
- [`src/http_api.rs`](/Users/ashray/code/amxv/computer-mcp/src/http_api.rs)
- [`README.md`](/Users/ashray/code/amxv/computer-mcp/README.md)
- [`gg/agent-outputs/computer-cli-quickstart-for-agents.md`](/Users/ashray/code/amxv/computer-mcp/gg/agent-outputs/computer-cli-quickstart-for-agents.md)

#### What to do

Finish the feature with the minimum observability needed to operate a shared daemon.

Add:

- structured logging when sessions are created, continued, killed, timed out, and removed
- log fields for internal session id, session handle prefix, transport origin, command summary, and cwd
- regression tests for:
  - unknown handle rejection
  - handle uniqueness
  - concurrent session independence
  - same-session continuation behavior
  - session cleanup after exit

Update docs to explain:

- one fixed endpoint can host multiple concurrent sessions
- running commands return a session handle
- `write_stdin` continues a specific session handle
- session ownership is capability-based, not numeric-id-based

#### Validation strategy

- targeted unit/integration tests for session lifecycle behavior
- docs/examples updated to the final contract
- no regressions in existing HTTP/MCP smoke tests

#### Risks / fallbacks

- Risk: logging too much command/output detail
- Fallback: log command summaries and identifiers, not full output bodies

## Cross-provider Requirements

This design must stay provider-agnostic.

That means:

- no Sprite-specific concurrency logic in the session runtime
- no requirement for multiple public URLs, multiple daemons, or multiple provider-level services
- the same concurrent session broker model must work on Sprites, Runpod, and a normal VM

Provider-specific deployment behavior may stay where it already belongs, but the session-ownership and concurrent-runtime design should live entirely in the core daemon.

## Recommended Implementation Notes

- Prefer landing the protocol contract and runtime refactor together in one implementation stream so the repo does not sit in a half-migrated state.
- Keep the public route structure unchanged.
- Favor an internal actor-style session runtime over broad shared locking.
- Do not add workspace sandboxing or OS-level separation in this feature.

## Final Validation Gate

Before handing the branch back to the lead, the implementation agent must run and report:

- `cargo test`
- `cargo clippy --all-targets -- -D warnings`

If either gate fails, the phase is not done.
