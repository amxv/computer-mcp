# Computer MCP VPS Design & Implementation Specification (v1)

## 1) Document Status
- Status: Draft for implementation
- Date: March 18, 2026
- Scope: Linux-only MVP for deployable remote MCP server with Codex-style tooling

This document consolidates all requirements and decisions discussed so far, plus implementation notes for building v1.

## 2) Product Vision
Build a remotely deployable MCP server that gives a coding agent practical remote-computer control on a VPS with minimal setup.

The MVP should feel like:
1. User SSHs into a VPS.
2. User runs a single public install command.
3. User starts the server from a CLI command.
4. Server runs in the background and is reachable over HTTPS on the public IP.
5. User manages API key from the same CLI.

## 3) Agreed Requirements

### 3.1 Platform & Deployment
- Linux only.
- Deployable on common VPS providers (DigitalOcean, Hetzner, RunPod, etc.).
- Zero/near-zero config install/start experience.
- Service must run in background and survive shell exit/reboot.

### 3.2 Endpoint & Auth
- Endpoint format: `https://<public_ip_of_vps>/mcp?key=<apikey>`
- Auth model:
  - Single global API key.
  - API key in query string only.
  - No OAuth.
  - No auth headers.
- Access level:
  - Full machine access (root-level capability intent).

### 3.3 Tooling Direction
- `apply_patch` must match Codex grammar and behavior as closely as practical.
- Prefer Codex-style shell tooling split:
  - `exec_command`
  - `write_stdin`
- This replaces a single monolithic `bash` tool as primary design.

### 3.4 TLS
- Preferred behavior:
  1. Attempt trusted Let's Encrypt IP cert.
  2. Fall back to self-signed cert automatically if not possible.
- Self-signed is acceptable.
- Domain support can be added later but is not required for v1.

### 3.5 CLI Name
- CLI command name: `computer-mcp`

### 3.6 Updates
- No auto-update required for v1.

## 4) Technical Findings Incorporated Into Design

### 4.1 `apply_patch` reuse from Codex
The official Codex repo contains a standalone Rust crate for apply patch:
- `codex-rs/apply-patch`
- Crate: `codex-apply-patch`
- Binary: `apply_patch`
- License: Apache-2.0

Implication:
- We can reuse this implementation directly or vendor/fork it in our Rust server stack.
- This is the best path to keep grammar and semantics aligned with Codex behavior.

### 4.2 Unified exec patterns from Codex
Official Codex has robust execution/session architecture in:
- `codex-rs/core/src/unified_exec`
- `codex-rs/utils/pty`

Useful to reuse conceptually:
- session-oriented process model (`session_id`)
- split tool model (`exec_command`, `write_stdin`)
- output buffering and truncation
- process cap + pruning strategy
- tty/non-tty behavior split

Not needed for this VPS MVP:
- sandbox orchestration
- approval policies
- permissions escalation flow
- guardian/network approval pipeline

### 4.3 Reference project reviewed
Reviewed `IgorWarzocha/pi-codex-conversion` as additional design input.
Its `exec_command` + `write_stdin` shape is compatible with desired UX and can inform a simpler MVP contract.

## 5) MVP Architecture

### 5.1 Components
1. `computer-mcpd` (daemon)
- MCP HTTP server
- Tool handlers
- Session/process manager
- TLS termination (direct or via embedded listener)

2. `computer-mcp` (CLI)
- install/start/stop/status/logs/key/tls operations
- systemd integration
- config management

3. Config + state storage
- Server config file
- TLS cert/key paths
- API key storage
- Runtime state (if needed)

### 5.2 Process Supervision
Use `systemd` (not `nohup`) for production reliability:
- background management
- auto-restart
- boot persistence
- logging through journald

## 6) API / Tool Specification (v1)

### 6.0 Finalized Simplification Decisions
- Keep two tools for shell execution:
  1. `exec_command`
  2. `write_stdin`
- Do not collapse to one `bash` tool.
- Keep API minimal to reduce model mistakes.
- Remove advanced/diagnostic fields from v1 tool contract.
- Keep `yield_time_ms` in tool inputs.
- Expose `timeout_ms` as an optional `exec_command` input in v1.
- Keep server-enforced timeout caps even when model provides `timeout_ms`.
- Rationale:
  - two-tool pattern is already familiar to Codex-class models
  - supports both short commands and long-running sessions cleanly
  - smaller schemas reduce parameter misuse and parsing complexity

### 6.1 Semantics: `yield_time_ms` vs timeout
- `yield_time_ms` controls **how long the server waits before responding to this call**.
- process timeout controls **how long the process may run before forced termination**.
- They are not equivalent and serve different purposes.

Example:
- If default `yield_time_ms = 10000` and command takes longer than 10 seconds:
  - response returns current `output` (possibly empty) and `session_id`
  - no `exit_code` yet
- If command finishes within the yield window:
  - response returns `output` and `exit_code`
  - no `session_id`

### 6.2 `exec_command` (minimal profile)
Purpose: run a command and return either final result or a running session reference.

Input fields:
- `cmd: string` (required)
- `yield_time_ms?: number`
- `workdir?: string`
- `timeout_ms?: number`

Output fields:
- `output: string`
- `session_id?: number` (present when still running)
- `exit_code?: number` (present when exited)

Behavior:
- The backend runs commands via `/bin/bash -lc`.
- Commands run with PTY-backed sessions for consistent interactive behavior.

### 6.3 `write_stdin` (minimal profile)
Purpose: continue a running session by polling and/or writing stdin bytes.

Input fields:
- `session_id: number` (required)
- `chars?: string` (empty/omitted means poll only)
- `yield_time_ms?: number`
- `kill_process?: boolean`

Output fields:
- `output: string`
- `session_id?: number` (present when still running)
- `exit_code?: number` (present when exited)

Behavior:
- `chars` sends bytes to the process stdin and then waits up to `yield_time_ms`.
- empty `chars` performs poll/wait only.
- `kill_process=true` terminates the target process and then returns output/exit state.
- If `kill_process=true`, server should ignore `chars` for that call.
- unknown/expired `session_id` returns explicit `Unknown process id` style error.

### 6.4 Stateful Session Semantics
- Shell state is session-scoped and persistent while the session is alive.
- Changes made in one call persist to the next call for the same `session_id`, including:
  - current working directory (`cd`)
  - environment variables (`export`)
  - shell-local state
- This satisfies workflows like:
  1. call A: change directory
  2. call B: run command without passing `workdir`
  3. result uses the directory from call A (same session)
- A different/new session does not inherit state from other sessions.

### 6.5 `apply_patch`
Purpose: file mutation using Codex-style patch grammar.

Requirements:
- Grammar and operational behavior should mirror official Codex apply patch tool.
- Reuse official `codex-apply-patch` implementation path to minimize drift.

## 7) Execution Runtime Design

### 7.1 Session lifecycle
- Generate numeric `session_id` for long-running sessions.
- Keep in-memory session map in daemon.
- Track:
  - process handle
  - command
  - last-used timestamp
  - output buffer state

### 7.2 Yield behavior
- Clamp `yield_time_ms` into safe bounds.
- Empty polls may use stronger minimum to avoid busy-loop patterns.
- Default `yield_time_ms` for `exec_command`: `10000 ms` (10 seconds).
- Default `yield_time_ms` for `write_stdin`: `10000 ms` (10 seconds).

### 7.3 Output buffering
- Use capped head/tail buffer strategy (Codex-style) to avoid unbounded memory.
- Apply token-based truncation approximation (`~4 chars/token`) for output caps.

### 7.4 Concurrency limits
- Cap max open sessions (recommended: 64, Codex-aligned).
- Prune least-recently-used sessions when cap exceeded (prefer exited sessions first).

### 7.5 Timeout handling
- `exec_command.timeout_ms` may override timeout per command.
- server clamps/validates provided timeout using configured min/max guardrails.
- Terminate process group when timeout is exceeded.
- Return timeout-influenced exit state in tool output.

## 8) TLS & Network Strategy

### 8.1 Default behavior (agreed)
1. Try Let's Encrypt IP certificate flow first.
2. If acquisition/configuration fails, issue self-signed cert and continue.

### 8.2 Why this is now viable
As of 2026, Let's Encrypt publicly supports IP certs under short-lived profile, and Certbot documents IP issuance flow.

### 8.3 Operational approach
- `computer-mcp tls setup` (invoked by install/start path) attempts:
  1. Certbot + short-lived IP cert.
  2. Automatic fallback self-signed cert with IP SAN.
- Cert paths persisted in config.

## 9) CLI Specification (v1)

Minimum explicitly requested:
1. install
2. start

Recommended full v1 command set (still lightweight):
1. `computer-mcp install`
2. `computer-mcp start`
3. `computer-mcp stop`
4. `computer-mcp restart`
5. `computer-mcp status`
6. `computer-mcp logs`
7. `computer-mcp set-key <value>`
8. `computer-mcp rotate-key`
9. `computer-mcp show-url`
10. `computer-mcp tls setup`

## 10) Config Design (proposed)

Path candidates:
- `/etc/computer-mcp/config.toml` (system config)
- `/var/lib/computer-mcp/` (runtime/data)

Example fields:
- `bind_host = "0.0.0.0"`
- `bind_port = 443`
- `api_key = "..."` (or pointer to key file)
- `tls_mode = "auto" | "letsencrypt_ip" | "self_signed"`
- `tls_cert_path = "..."`
- `tls_key_path = "..."`
- `max_sessions = 64`
- `default_exec_timeout_ms = 7200000`
- `max_exec_timeout_ms = 7200000`
- `default_exec_yield_time_ms = 10000`
- `default_write_yield_time_ms = 10000`

## 11) Security Model (explicitly accepted)

This system intentionally exposes high-privilege remote execution.

Implications:
- Compromise of key == full host compromise potential.
- Query-string key has leakage risk in logs, history, intermediaries.

Required mitigations for v1:
1. Redact `key` query parameter in all logs.
2. Disable verbose access logs by default.
3. Keep CLI warning banner documenting risk model.
4. Encourage firewall/IP allow-listing as optional hardening.

## 12) Implementation Plan

### Phase 1: Core server skeleton
- Build Rust MCP server with endpoint routing and auth check (`key` query param).
- Add system config loader and key management.
- Establish baseline error contract and JSON-RPC/MCP response mapping.

### Phase 2: Execution tools
- Implement session manager and process runtime.
- Add `exec_command` and `write_stdin` with codex-like response schema.
- Add timeout and output truncation logic.
- Implement kill semantics: `kill_process=true` uses graceful terminate first, then forced kill after grace window.
- Confirm response-shape contract:
  - running => `session_id` only (no `exit_code`)
  - finished => `exit_code` only (no `session_id`)

### Phase 3: Apply patch
- Integrate `codex-apply-patch` implementation.
- Expose `apply_patch` tool endpoint.
- Validate grammar/behavior parity against representative fixtures from the API reference and Codex-style usage.

### Phase 4: CLI + service management
- Implement `computer-mcp` CLI command surface (`install/start/stop/restart/status/logs/set-key/rotate-key/show-url/tls setup`).
- Install and manage systemd unit (`computer-mcpd.service`) for boot persistence and auto-restart.
- Ensure `computer-mcp start` is idempotent and provides actionable status output when already running.

### Phase 5: Public install bootstrap (missing work added)
- Add one-command installer script for SSH-first VPS UX.
- Script responsibilities:
  - detect Linux distro/arch (Ubuntu/Debian first-class for v1)
  - install prerequisites (`curl`, `ca-certificates`, `systemd`, optional `certbot`)
  - install `computer-mcp` + `computer-mcpd` binaries
  - create required dirs/config with secure permissions
  - install/enable systemd service
  - print next steps (`computer-mcp set-key`, `computer-mcp start`, `computer-mcp show-url`)
- Installer must be non-interactive by default and idempotent on re-run.

### Phase 6: TLS bootstrap + network readiness
- Implement auto TLS path (LE IP first, fallback self-signed).
- Persist cert state and restart service as needed.
- Validate HTTPS endpoint shape end-to-end:
  - `https://<public_ip>/mcp?key=<apikey>`
- Include network checks in CLI status (listening port, cert mode, reachable URL hint).

### Phase 7: Hardening + full QA for VPS deployment readiness
- Log redaction and key leak checks (query string key never appears in logs).
- Integration tests for session lifecycle and state persistence.
- End-to-end install/start/connect test on fresh Linux VM.
- Failure-mode tests:
  - expired/unknown session behavior
  - timeout handling
  - `kill_process` behavior
  - TLS fallback when LE fails
- Final release gate: all tests passing and documented deploy steps verified on a clean VPS.

## 13) Testing Strategy

### 13.1 Unit tests
- session id allocation and unknown session errors
- write/poll behavior with `write_stdin` (including empty poll calls)
- timeout behavior
- output truncation behavior
- process pruning policy

### 13.2 Integration tests
- `exec_command` returns `session_id` for long-running command
- `write_stdin` drives interactive tty command to completion
- `write_stdin` with `kill_process=true` terminates running process and returns exit state
- stateful session behavior:
  - `cd` in one call affects next command in same session
  - environment variable exported in one call is visible in later calls in same session
- `apply_patch` add/update/delete/move scenarios
- auth rejection without valid `key`

### 13.3 Deployment tests
- single-command install on fresh Linux host
- `computer-mcp start` creates reachable HTTPS endpoint
- fallback to self-signed when LE flow fails

### 13.4 Release-readiness matrix (required before phase completion)
- Clean VPS bootstrap test:
  - from empty host, run public install command
  - set API key and start service
  - successful MCP tool calls against public IP over HTTPS
- Regression test set:
  - `exec_command` quick completion
  - `exec_command` long-running -> returns `session_id`
  - `write_stdin` poll/write/kill flows
  - stateful session (`cd`, `export`) across calls
  - `apply_patch` parity fixture suite
- Operational test set:
  - reboot host -> service auto-starts
  - key rotation invalidates old key immediately
  - logs remain key-redacted

## 14) Decisions Made vs Pending

### Finalized
- Linux-only.
- Endpoint format with query key.
- Single global key.
- Full access model.
- `exec_command` + `write_stdin` tooling approach.
- Minimal tool contract (reduced fields) for v1.
- Running/finished signal model:
  - running -> `session_id` present, `exit_code` absent
  - finished -> `exit_code` present, `session_id` absent
- Session-scoped state is persistent (`cd`, env, shell context) while session is alive.
- `exec_command` supports optional `timeout_ms` with server-side caps.
- `write_stdin` supports optional `kill_process`.
- Codex-style `apply_patch` behavior reuse.
- TLS strategy: try LE IP cert, fallback self-signed.
- CLI name: `computer-mcp`.
- No auto-update for v1.
- `yield_time_ms` semantics distinct from process timeout.
- Tool-provided timeout override with server-managed caps.

### Pending clarification (minor)
1. Public binary distribution method for installer:
   - GitHub Releases prebuilt artifacts
   - or build-from-source fallback when artifact unavailable.

## 15) Notes on Reuse Boundaries

To align with "same functionality as Codex CLI" for this scope:
- Reuse/port execution-session patterns from Codex unified-exec.
- Reuse `codex-apply-patch` behavior directly.
- Do not pull in Codex policy stack (sandbox/approval/network guardian), since VPS mode intentionally runs high-trust.

This keeps behavior familiar to Codex users while preserving a much simpler deployable architecture.

## 16) Reference Sources
- Codex apply patch implementation:
  - https://github.com/openai/codex/tree/main/codex-rs/apply-patch
- Codex unified exec implementation:
  - https://github.com/openai/codex/tree/main/codex-rs/core/src/unified_exec
- Let's Encrypt IP cert announcements/docs:
  - https://letsencrypt.org/2026/01/15/6day-and-ip-general-availability
  - https://letsencrypt.org/2026/03/11/shorter-certs-certbot
  - https://letsencrypt.org/2025/07/01/issuing-our-first-ip-address-certificate
- Additional reference implementation reviewed:
  - https://github.com/IgorWarzocha/pi-codex-conversion/tree/master
