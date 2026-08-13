# Local Apple Silicon Runtime — Progress Ledger

## Workstream

Add Zodex Local as a persistent isolated Apple Silicon Linux execution target while preserving Zodex Sprite as a simultaneous first-class target and preserving the existing ChatGPT-facing command/stdin/patch MCP surface.

**Treat current source, tests, generated contracts, and docs as truth. Progress claims are evidence to verify, never proof.**

## Required artifacts

- [Accepted specification](./local-apple-silicon-runtime-spec-2026-08-13.md)
- [Current-state Sweep](./local-apple-silicon-runtime-sweep-2026-08-13.md)
- [Authoritative implementation plan](./local-apple-silicon-runtime-implementation-plan-2026-08-13.md)
- [First implementation agent prompt](./first-agent-prompt.md)
- [Subsequent implementation agent prompt](./subsequent-agent-prompt.md)

There is no separate repository-wide live harness file for this workstream. Phases 1–7 name their required real Apple/OpenAI/GitHub evidence directly. Never infer destructive GitHub authorization; use only an explicitly authorized disposable repository/ref when a live push is required.

## Repository policy for this workstream

- Repository: `amxv/zodex`
- Approved branch: `main`
- Expected checkout on this Sprite: `/workspace/repos/zodex`
- Always fetch first and fast-forward safely.
- Current remote code is authoritative; the planning SHA is only provenance.
- Never force-push, hard reset away newer work, rewrite shared history, clean another agent's files, or stage unrelated changes.
- If the worktree contains another agent's uncommitted work, preserve it and restrict your edits/staging accordingly.
- The canonical broad repository validation is `bash scripts/check.sh`; phase-specific positive evidence is still required before a phase can be complete.
- Update user-facing docs only when the corresponding capability actually exists on the branch.
- If a load-bearing plan assumption fails, add a plan Amendment immediately. Never weaken the accepted Local isolation/authority goals to get a phase green.

## Status vocabulary

Use only:

- `pending`
- `in_progress`
- `blocked`
- `complete`

A phase is `complete` only when its observable outcome, phase-specific positive evidence, relevant regression evidence, applicable live evidence, docs/generated-contract work, ledger update, coherent commit, normal push, post-push fetch/remote equality and clean-worktree verification are complete.

## Phase table

| Phase | Capability | Status | Completion commit | Evidence / next boundary |
| ---: | --- | --- | --- | --- |
| 1 | Local provider/state seam and truthful read-only status | complete | `a8a710846d71705dcb9a329ac6059290a8d42434` | Apple Silicon/macOS 26.3.1 with Apple Container 1.2.2 proved the real version, capability, machine-not-found and read-only status shapes match the implementation. |
| 2 | Persistent Local machine setup and privileged operator exec | pending | — | Requires Phase 1. Private-LAN denial is a hard live security gate; block/amend if it cannot be host-enforced. |
| 3 | Secure MCP ingress and durable TTL start/stop lifecycle | pending | — | Requires a Phase 2-ready safe Local machine and official current Secure MCP Tunnel compatibility. |
| 4 | GitHub mode target parity with `--local` | pending | — | Requires Local privileged guest transport; preserve one canonical YOLO policy and Sprite behavior. |
| 5 | Reset, reprovisioning and lifecycle recovery hardening | pending | — | Requires setup/start/target paths to exist; reset must preflight before deletion. |
| 6 | Public docs, packaging and Sprite-preserving product integration | pending | — | Document only behavior present on the current branch; preserve Sprite-first-class guidance. |
| 7 | Independent end-to-end acceptance, security and performance hardening | pending | — | Re-read full current plan/Amendments and prove all acceptance criteria from current code/live systems. |

## Current handoff

- **Last completed phase:** Phase 1.
- **Earliest incomplete phase:** Phase 2.
- **Phase title:** `Persistent Local machine setup and privileged operator exec`.
- **Observable boundary:** the read-only `zodex local status`, Local provider/state seams, deterministic Apple provider parsing/capability classification, private atomic Local state and shared duration grammar are implemented and validated. A live Apple Silicon/macOS 26.3.1 probe against Apple Container 1.2.2 confirmed the real version, required `--home-mount ... none` capability, machine-not-found error shape and truthful non-mutating status output.
- **Current blockers:** none for entering Phase 2. Phase 2 carries the highest-regret factual uncertainty: host/provider enforcement of public-Internet egress while denying macOS/private-LAN access. Treat that as a hard live gate and block/amend rather than weakening it to guest-only enforcement.
- **Plan Amendments affecting next phase:** none.
- **Prompt to use:** [`subsequent-agent-prompt.md`](./subsequent-agent-prompt.md).

## Durable ledger rules

After every completed or blocked phase:

1. update that phase's row immediately;
2. update **Current handoff** to the earliest genuinely incomplete boundary;
3. append a full progress entry using the format below;
4. if implementation invalidated a load-bearing assumption/approach, add the plan Amendment before handoff;
5. commit/push coherent safe work and record full pushed SHA(s);
6. fetch after push, prove local HEAD equals remote `main`, and verify the worktree is clean except for preserved pre-existing work belonging to another agent.

If a session stops partway through a phase, leave the phase `in_progress`. Record exactly what slice is pushed, what evidence actually exists and the next action. Do not call a partial vertical slice complete merely because it compiles.

### Progress entry format

```markdown
### YYYY-MM-DD — Phase N — `<status>`

- **Agent/session:** ...
- **Starting state:** branch/worktree actually inspected; SHA may be included as a coordinate.
- **Ending commit(s):** full pushed SHA(s).
- **Outcome:** what became observably true.
- **Files/areas changed:** concise paths and major boundaries.
- **Positive evidence:** phase-specific proof.
- **Regression evidence:** existing behavior re-proven.
- **Live evidence:** real-system evidence, or a concrete reason it was not applicable.
- **Documentation:** help/docs/generated-contract work and validation, or a concrete not-applicable reason.
- **Decisions made:** local implementation judgments that do not amend the plan.
- **Amendments:** plan Amendment link/reference or `none`.
- **Known defects/risks:** exact remaining problems.
- **Next handoff:** earliest incomplete boundary and what the next agent should inspect first.
```

Never put API keys, GitHub tokens, PEM contents, OpenAI tunnel runtime keys, private host data, local databases or raw sensitive live payloads in this ledger.

## Progress entries

### 2026-08-13 - Phase 1 - `complete`

- **Agent/session:** `mini_cascade` on the Apple Silicon macOS workspace checkout.
- **Starting state:** clean `main` at `20206f6e3481d9a0a77dbc5858c9f52115abac66`, exactly equal to fetched `origin/main`; no pre-existing uncommitted work was present.
- **Ending commit(s):** implementation/tests commit `085efc1aacb16f38c82587522c23d5ca5b4efee5`; live-evidence completion commit `a8a710846d71705dcb9a329ac6059290a8d42434`. The ledger-coordinate commit follows this entry and is reported in the final and AgentBox handoffs because a commit cannot contain its own SHA.
- **Outcome:** completed the required Phase 1 provider probe on the target Apple Silicon Mac. The fixed `zodex-local` machine does not exist, so status truthfully reports the target as not configured and does not adopt or create a machine.
- **Files/areas changed:** `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-progress.md` only; no runtime implementation changes were required.
- **Positive evidence:** on `arm64` macOS 26.3.1, `container system version --format json` returned Apple Container CLI 1.2.2 with the expected structured array and `appName`/`version` fields. `container machine create --help` exposes `--home-mount <home-mount>` with `ro`, `rw` and `none`. With the provider service running, `cargo run --quiet --bin zodex -- local status` exited 0 and printed `Local target: zodex-local`, `Configuration: not configured`, `Provider: ready (1.2.2)`, `Machine: not found` and `MCP access: inactive`.
- **Regression evidence:** the Phase 1 targeted Local/Sprite/operator/install tests and full `bash scripts/check.sh` gate are rerun from this Apple Silicon checkout before the completion commit is pushed.
- **Live evidence:** Apple Container system status reported its API server running at version 1.2.2. Raw `container machine inspect zodex-local` returned the expected not-found diagnostic. The Zodex status path classified that result as `Machine: not found`, exited successfully and left both `~/.config/zodex/local-target.json` and `~/.config/zodex/local-access-lease.json` absent. Starting the previously uninitialized provider service required installing its recommended default kernel with explicit operator authorization; no machine was created, started, stopped, adopted, destroyed or reconfigured.
- **Documentation:** the progress ledger records the live completion evidence. Public help and CLI tests remain limited to the shipped read-only `local status`; no later-phase commands or generated MCP contracts changed.
- **Decisions made:** Apple Container 1.2.2 matches all load-bearing Phase 1 parser and capability assumptions, so the implementation remains unchanged.
- **Amendments:** none.
- **Known defects/risks:** no Phase 1 defect remains. Phase 2 must prove that a host/provider-controlled boundary can allow public Internet while denying macOS/private-LAN access before Local setup can be accepted.
- **Next handoff:** begin Phase 2 at `Persistent Local machine setup and privileged operator exec`; read the current Phase 1 provider/state modules and the Phase 2 files and security gates before implementing machine lifecycle behavior.

### 2026-08-13 — Phase 1 — `blocked`

- **Agent/session:** first implementation agent on the Zodex Sprite checkout.
- **Starting state:** fetched and fast-forwarded clean `main` to `9d73de1eca617237ef3208b57c54cb307031fcf5`; no pre-existing uncommitted work was present.
- **Ending commit(s):** implementation/tests commit `085efc1aacb16f38c82587522c23d5ca5b4efee5`. The ledger-only handoff commit follows this entry and is reported in the final handoff because a commit cannot truthfully contain its own SHA.
- **Outcome:** added a public read-only `zodex local status` namespace while deliberately not exposing setup/start/exec/reset yet. Status reports the fixed `zodex-local` identity, not-configured/provisioning/ready host state, unsupported/missing/incompatible/ready provider state, observed machine running/stopped/resources/home-mount state when available, unmanaged/drift conditions, and inactive/active/expired access-lease state. Apple-specific provider probing/parsing and Local host persistence are isolated in dedicated modules; Sprite and MCP surfaces were not generalized or changed.
- **Files/areas changed:** `src/bin/zodex/{prelude.rs,dispatch.rs,credentials.rs,local_provider.rs,local_state.rs,mod.rs}`, `src/bin/zodex/tests/{part1.rs,part2.rs}`, and `tests/zodex_operator_cli.rs`.
- **Positive evidence:** `cargo test --bin zodex local_ -- --nocapture` passed 5 Local tests covering runtime platform classification, missing-vs-unsupported provider classification, current version/home-mount capability fixtures, fail-closed invalid Local state, and atomic/private state round-trips; `cargo test --bin zodex apple_machine_command_and_inspect_fixture_match_current_contract -- --nocapture` passed; `cargo test --bin zodex parse_push_grant_ttl_accepts_common_units -- --nocapture` passed and explicitly proves `2d`; `cargo test --test zodex_operator_cli -- --nocapture` passed 5 tests including Local help/status read-only behavior. A direct Linux execution with a temporary `HOME` printed `Configuration: not configured`, an actionable `linux/x86_64` unsupported-provider classification and `MCP access: inactive` without creating Local state files.
- **Regression evidence:** `cargo test --bin zodex sprite_registry_ -- --nocapture` passed all 3 explicit/env/single/ambiguous registry/path/upsert tests; GitHub mode help/TTL regressions passed in `tests/zodex_operator_cli.rs`; `cargo test --test install_script -- --nocapture` passed all 4 installer tests including non-root operator installation; `bash scripts/check.sh` passed `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the 1000-LOC source guard and the complete repository test suite.
- **Live evidence:** required Apple Silicon live evidence is **unavailable and not claimed**. This session is Linux/x86_64, `command -v container` is unavailable, and only `x86_64-unknown-linux-gnu` is installed in rustup. Current first-party Apple Container source/docs were re-checked at upstream source commit `5fcbeab5701bc5aa739285e2d6d3e97c863188ee`: `container machine inspect` emits JSON, `container machine create` supports `--home-mount` with `rw` default and `none`, and `container system version --format json` exposes structured CLI version data. The plan makes the real supported-Mac status/provider probe required Phase 1 evidence, so the phase remains blocked rather than being marked complete.
- **Documentation:** public Clap help and CLI tests were updated only for `zodex local status`. No setup/start/tunnel/GitHub-Local user docs were added because those capabilities do not exist yet. No MCP schema/generated model-facing contract files were changed.
- **Decisions made:** Local V1 host state lives in dedicated `~/.config/zodex/local-target.json` and `local-access-lease.json` records with schema/identity validation, atomic replacement and user-private permissions. Existing GitHub TTL parsing was factored to a provider-neutral duration helper while preserving existing GitHub error vocabulary/semantics. Provider version JSON is parsed rather than printed as an opaque blob, and observed home mounts are explicitly classified as isolated only when equal to `none`.
- **Amendments:** none; current Apple provider source still matches the Phase 1 planned command/status seams.
- **Known defects/risks:** Phase 1 cannot be completed until `zodex local status` is run read-only on the actual supported Apple Silicon/macOS + Apple Container environment and its real version/capability/inspect shape is verified. Phase 2 must still settle the harder host-controlled public-Internet-without-host/private-LAN enforcement gate before any Local setup/start can be accepted.
- **Next handoff:** remain in Phase 1. On a supported Apple Silicon Mac, fetch the pushed Phase 1 implementation, run the read-only `zodex local status` against the installed current Apple Container CLI, verify the reported provider version/capability and any observed `zodex-local` machine status/home-mount shape without mutation, and record that live evidence. If it passes, mark Phase 1 complete and only then begin Phase 2 at `Persistent Local machine setup and privileged operator exec`.
