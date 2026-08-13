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
| 2 | Persistent Local machine setup and privileged operator exec | in_progress | — | Provider/provisioning/identity/exec foundations are implemented behind an unexposed fail-closed network gate. Apple Container 1.2.2 hard-wires machines to its built-in NAT network; prove a host-controlled public-Internet-without-macOS/private-LAN policy on the target Mac before exposing setup/exec or marking ready. |
| 3 | Secure MCP ingress and durable TTL start/stop lifecycle | pending | — | Requires a Phase 2-ready safe Local machine and official current Secure MCP Tunnel compatibility. |
| 4 | GitHub mode target parity with `--local` | pending | — | Requires Local privileged guest transport; preserve one canonical YOLO policy and Sprite behavior. |
| 5 | Reset, reprovisioning and lifecycle recovery hardening | pending | — | Requires setup/start/target paths to exist; reset must preflight before deletion. |
| 6 | Public docs, packaging and Sprite-preserving product integration | pending | — | Document only behavior present on the current branch; preserve Sprite-first-class guidance. |
| 7 | Independent end-to-end acceptance, security and performance hardening | pending | — | Re-read full current plan/Amendments and prove all acceptance criteria from current code/live systems. |

## Current handoff

- **Last completed phase:** Phase 1.
- **Earliest incomplete phase:** Phase 2.
- **Phase title:** `Persistent Local machine setup and privileged operator exec`.
- **Observable boundary:** Phase 2 has a tested internal foundation for an embedded Ubuntu/systemd Local image, no-home-mount machine create/reconcile, token-preserving privileged provider exec/data transfer, nonsecret durable setup-source references, provider-private agent/publisher service units, loopback daemon configuration and a separate `zodex-tunnel` identity. The public `local setup`/`local exec` commands remain intentionally unregistered while network isolation is unproven.
- **Current blockers:** Apple Container 1.2.2 machine source attaches every container machine to the built-in default network, and that built-in network is `nat`/vmnet shared mode; the machine CLI exposes no alternate network selector. A target-Mac live investigation must prove a maintainable **host-controlled** policy that allows public Internet but denies macOS/private-LAN access before setup may mutate a machine or mark it ready. Guest-only firewalling is not acceptable. If no such host policy can be proven, Phase 2 must be marked `blocked` and the plan amended before later phases.
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

### 2026-08-13 — Phase 2 — `in_progress`

- **Agent/session:** subsequent implementation agent on the Zodex Sprite checkout.
- **Starting state:** fetched and fast-forwarded clean `main` to `d17bfe2d0882cccbf94a65f019b54930be001ba2`, incorporating the Mac agent's completed Phase 1 live evidence.
- **Ending commit(s):** the Phase 2 foundation commit containing this entry follows; its full pushed SHA is reported in the AgentBox/final handoff because a commit cannot truthfully contain its own SHA.
- **Outcome:** implemented a coherent, non-user-visible Phase 2 foundation while keeping the security gate fail-closed. Added an embedded Ubuntu 24.04/systemd machine recipe, deterministic no-home-mount create/resource reconciliation, root `container machine run` command construction that preserves argument tokens, stdin-based bounded file transfer, durable nonsecret setup-source references, runtime-installer-based guest provisioning, loopback-only Zodex daemon configuration, explicit `zodex-agent` and `zodex-publisher` systemd identities, a separate restricted `zodex-tunnel` identity/directories, post-provision verification logic and setup/reconcile classification that refuses unmanaged machine adoption. `zodex local setup` and `zodex local exec` are not registered in the public Clap tree yet because the required host network policy is not proven.
- **Files/areas changed:** `src/bin/zodex/local_provider.rs`, `local_state.rs`, new `local_setup.rs`, embedded `local_machine.Containerfile`, provider-private `local_zodexd.service`/`local_zodex_prd.service`, module wiring and Local unit tests.
- **Positive evidence:** `cargo test --bin zodex local_ -- --nocapture` passed 11 Local tests. New tests prove machine creation always passes `--home-mount none` and no mount/volume flags, resource reconciliation retains that invariant, operator exec keeps literal token boundaries, owned targets reconcile while unmanaged machines are rejected, the embedded guest setup uses runtime-only install/loopback binding/separate agent-publisher-tunnel identities without host PEM paths, conservative setup input validation, fail-closed ready-state source requirements and the network gate refusing progression while host isolation is unproven.
- **Regression evidence:** `cargo clippy --all-targets -- -D warnings`, the 1000-LOC source guard, Sprite registry tests, `tests/zodex_operator_cli.rs`, `tests/install_script.rs`, `tests/sprite_scripts.rs` and the full `bash scripts/check.sh` gate all passed. Existing Local help still exposes only `status`, so no incomplete setup/exec promise leaked into the CLI.
- **Live evidence:** no Phase 2 machine mutation was performed from this Linux Sprite. Current Apple Container source was re-checked at the exact 1.2.2 tag (`3c4c59cf6a1099c334f1ad6d485f79969e09f4b9`): `MachinesService` attaches machine configuration to `NetworkClient().builtin`, the built-in network is configured as `nat` with `container-network-vmnet`, and vmnet NAT maps to shared mode. The public machine create/run surface has no network selector. The real target-Mac host filtering mechanism and item-5 denial proof therefore remain outstanding and are not claimed.
- **Documentation:** no user docs or model-facing contracts changed. That is intentional because Phase 2's public setup/exec behavior is not yet safe to expose. The embedded machine/service artifacts are runtime implementation inputs, not user documentation.
- **Decisions made:** put the host-network isolation gate before setup state writes, image builds, machine creation or reconciliation, so an unproven network boundary cannot leave an apparently configured Local target. Preserve PEMs as operator source-path references only; transfer contents over provider stdin into private guest temp files and never persist them in host JSON. Keep setup/exec implementation internal until the live host boundary passes.
- **Amendments:** none yet. Apple Container's lack of a machine network selector narrows the search to a macOS/provider-side host filter but does not by itself prove the accepted network outcome impossible. Add an Amendment immediately if the target-Mac investigation disproves a viable host-controlled mechanism or selects a materially different provider arrangement.
- **Known defects/risks:** Phase 2 is not complete and no public setup/exec command exists yet. The hard unknown is whether Apple Container's shared NAT traffic can be reliably filtered on macOS per Local machine (including reboot/IP-change reconciliation) without trusting guest root and without breaking unrelated host/container traffic. The embedded machine image and full guest provisioning path also still require first live execution on the target Mac after that network mechanism is selected.
- **Next handoff:** continue Phase 2 on the Apple Silicon Mac. First investigate and live-prove a host-controlled network filter for the Apple Container 1.2.2 machine path: public GitHub/package egress must succeed while the macOS host/gateway and reachable private-LAN addresses fail. It must be enforceable/reconcilable before future tunnel activation and survive machine IP changes safely. If proven, implement that gate, expose `local setup`/`local exec`, run the complete Phase 2 live checklist and mark complete. If not provable, block Phase 2 and add the required architecture Amendment instead of using a guest firewall.

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
