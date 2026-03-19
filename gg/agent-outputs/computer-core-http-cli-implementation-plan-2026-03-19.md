# Computer Core + HTTP API + CLI Implementation Plan

Date: 2026-03-19

Scope: extract a transport-neutral core for `exec_command`, `write_stdin`, and `apply_patch`; expose it through the existing daemon as both MCP and HTTP; add a thin downloadable `computer` CLI; keep `publish-pr` local to the VPS.

## State of Current System

- `src/session.rs` already contains the real command/session behavior: PTY allocation, stateful session reuse, output truncation, live cwd reporting, idle timeout reset, timeout notices, kill semantics, and the running vs exited `ToolOutput` contract.
- `src/apply_patch.rs` already contains the real patch behavior and is transport-neutral today.
- `src/protocol.rs` already provides shared request structs and the command response model, but there is no dedicated service layer between those types and the transport adapters.
- `src/server.rs` still owns both daemon/router setup and the direct calls into `SessionManager` and `apply_patch::apply_patch`, so the service boundary effectively lives inside the MCP adapter.
- `src/server.rs` currently serves `/health` and `/mcp`, keeps `/mcp` behind query-string API-key auth, and reuses the same Axum app for both the TLS listener and the optional plain-HTTP listener driven by `http_bind_port`.
- `src/publisher.rs` is already isolated behind a Unix socket and does not need new transport work for this feature.
- `src/bin/computer-mcp.rs`, `.github/workflows/release.yml`, `scripts/install.sh`, `Dockerfile`, and `Dockerfile.runpod` all hardcode the current three-binary world, so adding `computer` is partly a packaging change, not just a Rust change.
- Tests are strongest at the session and patch layers today. `src/session.rs` and `src/apply_patch.rs` have behavior-rich unit tests. `src/server.rs` only verifies tool registration and annotations, and there are no current HTTP transport tests for the future `/v1/...` surface.

## State of Ideal System

- A new transport-neutral service module is the single execution boundary for `exec_command`, `write_stdin`, and `apply_patch`.
- `src/server.rs` becomes router/auth/listener composition code instead of the canonical home of computer-operation logic.
- `computer-mcpd` serves `/health`, `/mcp`, and `/v1/...` from the same process and the same shared service instance.
- MCP remains contract-stable for current clients.
- The HTTP API exposes the same three operations with the same behavior and field meanings.
- A standalone `computer` binary talks to the HTTP API with flags, environment variables, or an optional saved connection profile.
- `publish-pr` remains outside the remote API and continues to run through the existing local publisher daemon model.
- Release/install/image flows can ship the new client without breaking existing server rollout paths.

## Cross-Provider Requirements

- The service layer must be the only place where exec/write/apply semantics live.
- `ToolOutput` field meanings must stay identical across direct service tests, MCP, and HTTP.
- Session behavior from `src/session.rs` must not fork by transport.
- `apply_patch` path-rewrite behavior must not fork by transport.
- HTTP auth must reuse the existing `api_key`; do not add a second secret.
- `/health` should remain stable because existing install/status guidance already references it.
- Adding a downloadable `computer` asset must not make `scripts/install.sh` pick the wrong release archive for server installs.

## Plan Phases

### Phase 1. Extract A Shared Computer Service

Files to read before starting:

- `src/server.rs`
- `src/session.rs`
- `src/apply_patch.rs`
- `src/protocol.rs`
- `src/lib.rs`

What to do:

- Add `src/service.rs` and export it from `src/lib.rs`.
- Move the transport-neutral shared state out of the MCP handler type and into a service struct, for example a `ComputerService` that owns `Arc<Config>` plus the session manager state required by `exec_command` and `write_stdin`.
- Give the service three public entrypoints matching the feature scope: `exec_command`, `write_stdin`, and `apply_patch`.
- Keep `SessionManager` and `apply_patch::apply_patch` as the behavior owners in this phase. The service should delegate to them rather than reimplement them.
- Extract router-building seams from `src/server.rs` as needed so future HTTP tests can exercise handlers without binding real sockets. This is important because `run_server` currently mixes listener setup with route construction.

Validation strategy:

- Preserve the existing `src/session.rs` and `src/apply_patch.rs` tests unchanged.
- Add direct service-level tests that call the new service without MCP or HTTP and prove one command path, one session-continuation path, and one patch path behave identically to the current lower-level behavior.
- Run focused Rust tests for `session`, `apply_patch`, and the new `service` module before moving on.

Risks / fallbacks:

- The biggest risk is accidental behavior drift while moving code. Keep the first version as a thin delegating wrapper, not a deep rewrite.
- If router extraction starts to entangle transport code too early, limit this phase to the service boundary and postpone full app-construction cleanup to Phase 2.

### Phase 2. Rewire MCP To The Service And Lock Parity

Files to read before starting:

- `src/server.rs`
- `src/service.rs`
- `src/protocol.rs`
- `src/session.rs`

What to do:

- Update the MCP handler implementation so each tool delegates to the shared service instead of touching `SessionManager` or `apply_patch` directly.
- Preserve the existing tool names, descriptions, annotations, and MCP route/auth behavior.
- Keep `ComputerMcpService` as an MCP adapter type only. It should stop being the canonical home of operation logic.
- If Phase 1 did not fully separate app construction from listener startup, finish that separation here so both MCP and future HTTP routes can be tested without spawning the whole daemon.

Validation strategy:

- Keep the current `src/server.rs` tool-registration and annotation tests passing.
- Add parity tests that compare service results with MCP-adapter-invoked results for representative exec, write, and patch cases where practical.
- Add or extend unit tests around any refactored auth/helper functions so `/mcp?key=...` behavior stays stable.

Risks / fallbacks:

- `rmcp` macro usage can make abstractions awkward. If a generic helper complicates the MCP adapter, keep small per-tool wrapper methods and centralize only the actual service call.
- Do not rename protocol fields or tool names in this phase. That would blur transport parity failures with unrelated API churn.

### Phase 3. Add The HTTP API On The Existing Daemon

Files to read before starting:

- `src/server.rs`
- `src/service.rs`
- `src/protocol.rs`
- `src/config.rs`
- `src/apply_patch.rs`

What to do:

- Add a dedicated HTTP adapter module, preferably `src/http_api.rs`, and export it from `src/lib.rs` if that helps keep `src/server.rs` thin.
- Mount protected `/v1/exec-command`, `/v1/write-stdin`, and `/v1/apply-patch` routes on the same Axum app that already serves `/health` and `/mcp`.
- Reuse the existing request structs from `src/protocol.rs`.
- Add bearer-token auth middleware for the `/v1` routes using the existing `config.api_key`.
- Keep `/health` public and keep `/mcp` on query-string auth.
- Make sure both the TLS listener and the optional plain-HTTP listener expose the same route set by sharing the same built app.
- Decide one explicit HTTP response shape for `apply_patch`. If a raw JSON string is awkward, add a minimal typed wrapper without changing the underlying patch result content or error text.

Validation strategy:

- Add in-memory Axum handler tests for:
  - authorized and unauthorized `/v1/...` requests
  - `exec-command` returning running vs exited states
  - `write-stdin` continuing a real session
  - `apply-patch` preserving current relative-path semantics
- Compare representative HTTP responses against direct service results to prove semantic parity.
- Keep existing `/health` behavior stable in tests because install/status flows already rely on it.

Risks / fallbacks:

- The main risk is transport-specific behavior drift, especially around `apply_patch` response formatting. If needed, prioritize semantic parity over perfect response-shape elegance in the first pass.
- Avoid creating a second config knob for HTTP auth. Reusing `api_key` keeps rollout risk lower.

### Phase 4. Add The `computer` HTTP CLI

Files to read before starting:

- `Cargo.toml`
- `src/protocol.rs`
- `src/bin/computer-mcp.rs`
- `src/service.rs`
- `src/http_api.rs`

What to do:

- Add a new binary target in `Cargo.toml` for `src/bin/computer.rs`.
- Add `src/client.rs` if shared HTTP request logic will otherwise be duplicated across CLI commands or tests.
- Implement command resolution with this precedence:
  1. explicit `--url` / `--key`
  2. `COMPUTER_URL` / `COMPUTER_KEY`
  3. a saved single-target profile created by `connect`
- Implement these subcommands:
  - `connect`
  - `disconnect`
  - `exec-command`
  - `write-stdin`
  - `apply-patch`
- Keep `connect` and `disconnect` convenience-only. Stateless one-shot calls with flags or environment variables must work from the first usable version.
- Store the saved profile in a user-scoped config path. XDG-style storage is the simplest fit unless a repo standard requires something else.
- Use the existing `reqwest` dependency for the client path instead of introducing a second HTTP stack.
- Do not add a `publish-pr` command to `computer` in this first pass.

Validation strategy:

- Add CLI parsing and resolution tests for flag/env/profile precedence.
- Add tests for `connect` and `disconnect` persistence behavior using temporary directories or an overridable config path.
- Add one smoke test that exercises the client against a local in-process HTTP app or a lightweight test server and proves `exec-command` round-trips correctly.

Risks / fallbacks:

- The most likely risk is letting the convenience profile become the primary design center. Keep the stateless invocation path working first, then layer persistence on top.
- If cross-platform profile-path handling becomes noisy, keep the first pass conservative and Linux/XDG-friendly while preserving flags/env behavior everywhere.

### Phase 5. Update Release, Install, And Image Packaging Safely

Files to read before starting:

- `Cargo.toml`
- `.github/workflows/release.yml`
- `.github/workflows/container-release.yml`
- `scripts/install.sh`
- `tests/install_script.rs`
- `Dockerfile`
- `Dockerfile.runpod`

What to do:

- Extend release builds to compile the new `computer` binary.
- Publish `computer` in a way that preserves the existing server artifact contract. The safest path is:
  - keep the current `computer-mcp-...` server archive
  - add a separate standalone client asset for `computer`
- Update `scripts/install.sh` so its release-asset lookup explicitly prefers the server archive instead of matching any tarball that happens to contain the target triple. This matters because the current lookup logic can become ambiguous once a client-only asset exists.
- Decide whether container images should also include `computer`. If yes, copy it into both Docker images explicitly because both Dockerfiles currently enumerate copied binaries.
- Keep `computer-mcp install` focused on server-side install behavior. Do not make server bootstrap depend on the new client binary.

Validation strategy:

- Update or extend `tests/install_script.rs` so the installer contract still covers the expected snippets and any new asset-selection assumptions.
- Run a local release build that includes all bins and verify the expected files exist.
- Verify that any Dockerfile change still references the right built binary paths.

Risks / fallbacks:

- The biggest packaging risk is silently breaking tagged VPS installs by letting `scripts/install.sh` download the wrong archive. Treat installer archive selection as a required compatibility fix, not an optional cleanup.
- If shipping a separate client asset is too disruptive in the first pass, include `computer` in the existing server archive temporarily and defer standalone-client packaging until installer selection is made explicit.

### Phase 6. Run End-To-End Parity Verification

Files to read before starting:

- `src/service.rs`
- `src/server.rs`
- `src/http_api.rs`
- `src/bin/computer.rs`
- the transport tests added in earlier phases

What to do:

- Add a final verification pass that proves the same representative workflows succeed through:
  - direct service calls
  - MCP metadata/adapter coverage
  - HTTP requests
  - the `computer` CLI
- Focus on one exited command, one long-running session with `write-stdin`, and one `apply_patch` operation with a relative path.
- Confirm that no code path exposes `publish-pr` as a new remote HTTP surface.

Validation strategy:

- Run the Rust test subsets added in earlier phases together.
- Run the repo’s standard Rust gate once the implementation is complete:
  - `cargo test`
  - `cargo clippy --all-targets -- -D warnings`
- Do one manual local smoke check of the new CLI against a running daemon if the implementation branch allows it.

Risks / fallbacks:

- End-to-end verification can surface route-shape or packaging issues late. If that happens, fix the shared service and HTTP client seams first instead of papering over discrepancies in transport-specific code.
