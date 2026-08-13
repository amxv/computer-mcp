# Local Apple Silicon Runtime — Progress Ledger

## Workstream

Add Zodex Local as a persistent isolated Apple Silicon Linux execution target while preserving Zodex Sprite as a simultaneous first-class target and preserving the existing ChatGPT-facing command/stdin/patch MCP surface.

**Treat current source, tests, generated contracts, and docs as truth. Progress claims are evidence to verify, never proof.**

## Required artifacts

- [Accepted specification](./local-apple-silicon-runtime-spec-2026-08-13.md)
- [Current-state Sweep](./local-apple-silicon-runtime-sweep-2026-08-13.md)
- [Authoritative implementation plan](./local-apple-silicon-runtime-implementation-plan-2026-08-13.md)
- [Retired Phase 1 implementation prompt](./first-agent-prompt.md)
- [Cloud implementation agent prompt](./subsequent-agent-prompt.md)
- [Local integration agent prompt](./local-integration-agent-prompt.md)

Phases 2 through 6 are cloud implementation phases. Their Apple/OpenAI/GitHub live checklists are intentionally deferred to Phase 7, which is the consolidated Apple Silicon integration, repair and acceptance phase. Never infer destructive GitHub authorization; use only an explicitly authorized disposable repository/ref when a live push is required.

## Repository policy for this workstream

- Repository: `amxv/zodex`
- Approved branch: `main`
- Current Apple Silicon checkout: `/Users/ashray/code/amxv/zodex`
- Always fetch first and fast-forward safely.
- Current remote code is authoritative; the planning SHA is only provenance.
- Never force-push, hard reset away newer work, rewrite shared history, clean another agent's files, or stage unrelated changes.
- If the worktree contains another agent's uncommitted work, preserve it and restrict your edits/staging accordingly.
- The canonical broad repository validation is `bash scripts/check.sh`; phase-specific deterministic positive evidence is required before a cloud implementation phase can be complete.
- Update user-facing docs only when the corresponding capability actually exists on the branch.
- If a load-bearing plan assumption fails, stop, capture evidence and obtain any required user decision. Then rewrite the affected canonical spec/sweep/plan/prompt sections in place. Never weaken the accepted Local isolation/authority goals to get a phase green.

## Status vocabulary

Use only:

- `pending`
- `in_progress`
- `blocked`
- `complete`

For Phases 2 through 6, `complete` means the cloud implementation boundary, deterministic positive evidence, relevant regressions, applicable docs/generated-contract work and an explicit deferred-integration checklist are complete and pushed cleanly. It does not claim live Apple/OpenAI/GitHub acceptance. Phase 7 alone requires the consolidated live evidence, integration fixes and all final acceptance criteria before it can be `complete`.

## Phase table

| Phase | Capability | Status | Commit coordinate | Evidence / next boundary |
| ---: | --- | --- | --- | --- |
| 1 | Local provider/state seam and truthful read-only status | complete | `a8a710846d71705dcb9a329ac6059290a8d42434` | Apple Silicon/macOS 26.3.1 with Apple Container 1.2.2 proved the real version, capability, machine-not-found and read-only status shapes match the implementation. |
| 2 | Persistent Local machine setup and privileged operator exec | complete | `dbb4262b5541d0b89998864bb1fd61f6e872022f` | Cloud implementation and deterministic contracts complete; real Apple machine/network/reboot/bypass evidence is explicitly deferred to Phase 7. |
| 3 | Secure MCP ingress and durable TTL start/stop lifecycle | complete | `77a720be551bfb666bcfc71e56f0190d6eb26e42` | Cloud implementation, secret/service contracts and injected lease lifecycle evidence complete; live tunnel connection, sleep/expiry and Mac runtime behavior are explicitly deferred to Phase 7. |
| 4 | GitHub mode target parity with `--local` | pending | — | Cloud: preserve one canonical YOLO policy and prove Local/Sprite transport behavior deterministically; defer authorized live push to Phase 7. |
| 5 | Reset, reprovisioning and lifecycle recovery hardening | pending | — | Cloud: implement preflight/recovery through injected failures; defer destructive Local-machine exercise to Phase 7. |
| 6 | Public docs, packaging and Sprite-preserving product integration | pending | — | Cloud: complete final command/help/docs/embedded-asset/package contracts; defer clean Mac artifact smoke to Phase 7. |
| 7 | Apple Silicon integration, live repair and final acceptance | pending | — | Local: execute every deferred checklist in dependency order, fix live defects, and prove all acceptance criteria. |

## Current handoff

- **Last completed phase:** Phase 3.
- **Earliest incomplete phase:** Phase 4.
- **Phase title:** `GitHub mode target parity with --local`.
- **Observable boundary:** Phase 3 is cloud-complete at `77a720be551bfb666bcfc71e56f0190d6eb26e42`. Local setup now requires a dedicated Secure MCP Tunnel ID plus runtime-key file, installs the checksum-pinned stable Linux ARM64 tunnel client into the guest, keeps its runtime key in a `zodex-tunnel`-only file, leaves the tunnel disabled/inactive after setup, and places the tunnel service in the same restricted namespace as `zodexd`/`zodex-prd`. `zodex local start --ttl` reconciles stale access, verifies the runtime boundary, proves tunnel readiness before persisting an absolute-expiry generation lease, and arms a host launchd worker. `zodex local stop` and expiry share tunnel-first revocation logic. Status distinguishes operator-exec machine state from actual MCP/tunnel access. The model-facing registration is asserted to remain exactly `exec_command`, `write_stdin`, and `apply_patch`.
- **Current blockers:** none. A cloud agent can begin Phase 4 by extracting only the guest-manipulation transport seam needed by GitHub mode, keeping the canonical YOLO policy/publisher validation unchanged, then adding explicit `--local` selection to `github mode yolo/default/status`. Authorized live GitHub push remains Phase 7.
- **Canonical plan status:** the namespace decision is folded directly into the spec, sweep, implementation plan and continuation prompt. There are no overlay amendments to reconcile.
- **Prompt to use:** [`subsequent-agent-prompt.md`](./subsequent-agent-prompt.md).
- **Final local prompt:** after Phases 2 through 6 are cloud-complete, use [`local-integration-agent-prompt.md`](./local-integration-agent-prompt.md) for Phase 7.

## Durable ledger rules

After every completed or blocked phase:

1. update that phase's row immediately;
2. update **Current handoff** to the earliest genuinely incomplete boundary;
3. append a full progress entry using the format below;
4. if implementation invalidated a load-bearing assumption/approach, capture the evidence and required user decision, then update the canonical package in place before handoff;
5. commit/push coherent safe work and record full pushed SHA(s);
6. fetch after push, prove local HEAD equals remote `main`, and verify the worktree is clean except for preserved pre-existing work belonging to another agent.

If a session stops partway through a phase, leave the phase `in_progress`. Record exactly what slice is pushed, what evidence actually exists and the next action. A cloud phase may complete without live access, but not merely because it compiles: its deterministic positive/failure contracts, regressions and deferred checklist must all be complete.

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
- **Deferred integration evidence:** exact Phase 7 live scenarios carried forward, plus any real-system evidence already available.
- **Documentation:** help/docs/generated-contract work and validation, or a concrete not-applicable reason.
- **Decisions made:** local implementation judgments and any approved canonical-plan update.
- **Plan updates:** canonical package sections changed, or `none`.
- **Known defects/risks:** exact remaining problems.
- **Next handoff:** earliest incomplete boundary and what the next agent should inspect first.
```

Never put API keys, GitHub tokens, PEM contents, OpenAI tunnel runtime keys, private host data, local databases or raw sensitive live payloads in this ledger.

## Progress entries

Phase 1 entries below are immutable historical records. Their forward-looking Phase 2 predictions are superseded by **Current handoff**, the current Phase 2 entry and the canonical implementation plan.

### 2026-08-13 — Phase 3 — `complete`

- **Agent/session:** subsequent implementation agent on the Zodex Sprite checkout, continuing directly from the completed Phase 2 boundary.
- **Starting state:** clean pushed `main` at `df784f38189543245709e0399d627bef59c29e16`, with Phase 2 implementation at `dbb4262b5541d0b89998864bb1fd61f6e872022f`; fetched `origin/main` before implementation/push coordination and found no remote divergence.
- **Ending commit(s):** implementation/tests commit `77a720be551bfb666bcfc71e56f0190d6eb26e42`; the ledger-coordinate commit follows this entry because a commit cannot contain its own SHA.
- **Outcome:** completed the cloud Secure MCP ingress and durable TTL lifecycle boundary. `local setup` now takes a pre-created tunnel ID/runtime-key path, installs a checksum-pinned `tunnel-client` `v0.0.11` Linux ARM64 artifact plus exact version marker, protects the runtime key as `zodex-tunnel:zodex-tunnel` mode `0600`, writes a root-owned tunnel config that references the key with `file:` rather than argv, attaches the tunnel service to the existing restricted `zodex-agent` namespace and deliberately leaves it disabled/inactive. `local start --ttl` is the only public ingress-enabling path and requires a finite TTL; it reconciles expired/pending access first, verifies the no-home-mount/network/service/secret boundary, proves local MCP health and tunnel `/readyz`, persists a generation-tagged absolute expiry only after readiness, and installs a generation-aware launchd worker on the host. Renewal replaces the generation so stale workers cannot stop a newer lease. `local stop` and expiry use the same tunnel-first then machine-stop revocation authority and persist truthful partial-failure state. `local status` now reports guest service/tunnel health, lease truth and operator-exec independence without changing runtime state.
- **Files/areas changed:** new `src/bin/zodex/local_tunnel.rs`, `local_lifecycle.rs`, `local_zodex_tunnel.service`, and `tests/part4.rs`; extended Local setup/state/provider CLI wiring and machine recipe; strengthened `src/server.rs` registration coverage; extended `tests/zodex_operator_cli.rs` and prior Local fixtures.
- **Positive evidence:** primary-source re-check confirmed current stable OpenAI `tunnel-client` `v0.0.11`, Linux ARM64 release availability, `file:` API-key references, restricted runtime-key permission requirements, exact `tunnel_` plus 32-lowercase-hex tunnel-ID grammar and `/readyz` readiness. The downloaded official `v0.0.11` Linux ARM64 archive matched SHA-256 `d8bba47b2a723799a372b0b87d7e4d69304093d3a28837237315fe5406d97e77`; setup records and verifies that version on-guest. Apple launchd primary documentation was re-checked for `ProgramArguments`, `RunAtLoad` and conditional `KeepAlive`. Operator unit coverage reached 101/101 and proves required TTL grammar, unsafe/pre-tunnel setup refusal before lifecycle side effects, pinned tunnel URL/hash/version, key-not-in-argv, disabled/no-auto-ingress setup, same namespace/capability-free tunnel unit, start ordering, renewal generation replacement, expired-lease reconciliation, tunnel-start and supervisor-arm fail-closed rollback, tunnel-first manual/expiry ordering, truthful machine-stop partial failure plus later reconciliation, both-stop-failed possibly-active state, absolute-time generation-checked worker behavior, and launchd plist contents without tunnel/runtime secrets.
- **Regression evidence:** `tests/zodex_operator_cli.rs` passed 7/7 including hidden worker/no-reset help, mandatory `--ttl` and no unlimited/no-TTL mode. `src/server.rs` now asserts the complete sorted MCP registration set equals exactly `apply_patch`, `exec_command`, `write_stdin`. The canonical `bash scripts/check.sh` passed cleanly: formatting, `cargo clippy --all-targets -- -D warnings`, source-file LOC guard, 83 library tests, 101 operator tests and every integration target. Existing Sprite, HTTP, session and CLI behavior remained green.
- **Deferred integration evidence:** Phase 7 must run first and repeated setup on the real Apple Silicon machine and prove the pinned `v0.0.11` ARM64 tunnel binary/version/checksum plus runtime-key/config ownership; connect the dedicated Local tunnel/app identity through ChatGPT with a short TTL; while access is active prove tunnel, daemon, publisher and representative agent child share the restricted namespace/inode with no provider interface; prove public tunnel/GitHub traffic while native Mac/vmnet/private-LAN/link-local/reserved/multicast and all IPv6 remain unreachable; through MCP run `id`/`pwd`, long-running `exec_command` plus `write_stdin` and a targeted `apply_patch`; manually stop during a running session and prove endpoint loss plus non-resumable pre-stop session handle after restart; let a short TTL expire after the initiating CLI exits and prove autonomous tunnel revocation plus machine stop; renew before expiry and prove the older generation cannot revoke the renewal; sleep the Mac across expiry and prove wake reconciliation remains revoked; reboot while a nominal lease would otherwise still be valid and prove access is not automatically re-exposed; and boot the machine through `local exec` while proving MCP remains inactive. Exercise the partial-stop repair path live if practical.
- **Documentation:** public Clap help now truthfully exposes `local start --ttl` and `local stop` in addition to setup/exec/status, while the launchd worker is hidden and reset remains absent for Phase 5. Broad user-facing Local documentation remains Phase 6 because GitHub target parity/reset/release packaging are not complete. No model-facing tool descriptions/schemas/annotations were changed; an exact registration-set assertion was added instead.
- **Decisions made:** pin the current stable tunnel client rather than fetching `latest`; record a guest version marker so later binary drift is fail-closed; pass only a host source path—not runtime-key contents—in Local host records; derive the namespace-local query-key MCP URL inside the privileged guest setup step; use one fixed host launchd label whose ProgramArguments contain only the current executable and random lease generation; cap worker sleeps at five seconds while using absolute wall-clock expiry so sleep/restart reconciliation does not extend leases; let an expiry worker exit successfully after revocation without self-unloading, while manual stop/setup reconciliation can remove the LaunchAgent; classify tunnel-stopped/machine-stop-failed as MCP inaccessible with durable machine-stop reconciliation pending, and classify both-stop-failed as possibly active/revocation pending.
- **Plan updates:** none. The canonical Phase 3 plan already required pinned tunnel assets, secret-file isolation, a generation-aware host supervisor, explicit finite TTL, shared tunnel-first revocation and live Phase 7 acceptance.
- **Known defects/risks:** no cloud Phase 3 defect is known. This environment cannot execute Apple launchd, Apple Container or a live OpenAI tunnel, so launchd domain semantics, systemd namespace attachment for the real tunnel process, actual `/readyz`, ChatGPT connector reachability, Mac sleep/reboot timing and autonomous expiry remain unaccepted until Phase 7. The setup input intentionally requires a pre-created dedicated tunnel/runtime credential rather than silently creating control-plane resources.
- **Next handoff:** begin Phase 4 `GitHub mode target parity with --local`. Read `github_mode.rs`, shared duration/config transport, publisher validation, `zodexd` git-remote paths, Sprite `run_sprite_exec`, current Local privileged exec/write primitives and the Phase 4 sweep landmines. Extract only the operator-side guest manipulation seam, preserve one `GithubModeRecord` and publisher validation implementation, add mutually exclusive explicit `--local` selection to yolo/default/status, and defer destructive/authorized live push proof to Phase 7.

### 2026-08-13 — Phase 2 — `complete`

- **Agent/session:** subsequent implementation agent on the Zodex Sprite checkout.
- **Starting state:** fetched and fast-forwarded clean `main` from the Phase 2 foundation to `6a3a5b95d64e4655bbbea308b27aca2fdef06494`; no pre-existing uncommitted work was present.
- **Ending commit(s):** implementation/tests commit `dbb4262b5541d0b89998864bb1fd61f6e872022f`; the ledger-coordinate commit follows this entry because a commit cannot contain its own SHA.
- **Outcome:** completed the cloud Phase 2 boundary and exposed `zodex local setup`, `zodex local exec` and the existing read-only `status`. Setup now provisions/reconciles the persistent no-home-mount Apple Container machine, installs a root-owned named Linux network namespace with only loopback plus a dedicated veth, installs interface-scoped nftables filter/NAT tables, uses public-only IPv4 resolver selection, attaches the agent and publisher services to the same namespace, strips their capabilities, and records a versioned network-policy identity in durable ready state only after verification. Operator exec remains provider-root execution in the trusted root namespace, verifies the setup/home-mount/network identity first and does not grant MCP ingress.
- **Files/areas changed:** `src/bin/zodex/{prelude.rs,dispatch.rs,mod.rs,local_provider.rs,local_state.rs,local_setup.rs}`, new `local_network.rs`, embedded `local_agent_network.sh`/`.service`, the Local daemon/publisher units, Local machine recipe, Local unit tests and `tests/zodex_operator_cli.rs`.
- **Positive evidence:** `cargo test --bin zodex local_ -- --nocapture` passed 15 Local tests. They prove fixed no-home-mount create/reconcile, literal argv boundaries, managed-vs-unmanaged setup classification, runtime installer/loopback/separate identities, conservative inputs, versioned network identity, the reviewed IPv4 deny set, interface-scoped root-input/forward/NAT rules, IPv6 disablement, public DNS fallback, no global ruleset flush, shared namespace/capability-free systemd units, fail-closed service ordering, unprivileged namespace exec token boundaries, atomic private state and invalid-ready-state rejection. `bash -n src/bin/zodex/local_agent_network.sh` passed. The generated nftables syntax/priority/policy forms were cross-checked against current Netfilter nftables documentation, and the deny set was cross-checked against the current IANA IPv4 Special-Purpose registry plus the plan-required multicast range.
- **Regression evidence:** `cargo test --test zodex_operator_cli -- --nocapture` passed 6 tests including setup/exec/status help and unsupported-platform fail-before-state-mutation behavior. The unchanged library suite passed 83/83 serially after one transient parallel PTY-harness hang; a clean retry of the canonical `bash scripts/check.sh` then passed exactly, including `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the 1000-LOC source guard, the parallel 83-test library suite, 87 operator tests and every integration-test target. Sprite/runtime behavior remained green.
- **Deferred integration evidence:** Phase 7 must, on the real Apple Silicon target, run first setup and idempotent setup; prove the machine still has no host-home mount; inspect the `zodex-agent` namespace and show only loopback plus the dedicated veth; prove `zodexd`, `zodex-prd` and later the tunnel all join that namespace while operator exec remains in root; prove public IPv4 DNS/GitHub HTTPS works; prove vmnet gateway/native Mac/private-LAN/link-local/reserved/multicast destinations and all IPv6 fail; attempt unprivileged interface/address/route/nftables/namespace/sudo escapes and confirm failure; reboot and prove topology/policy/services reconcile before model access; prove `local exec` remains available without starting MCP ingress; and prove workspace/Git/reader/publisher state persists across stop/start/reboot.
- **Documentation:** public Clap help and CLI integration tests now expose only the capabilities that exist: `local setup`, `local exec` and `local status`. User-facing guide work remains Phase 6 because Local ingress/start/stop/reset/GitHub parity do not exist yet. No MCP tool schema or generated model-facing contract changed.
- **Decisions made:** use one fixed policy identity (`policy_version = 1`, namespace `zodex-agent`, fixed veth names) in ready state; make the Local-owned nftables tables replaceable/inspectable without flushing unrelated rules; deny the complete reviewed IANA special-purpose IPv4 set (including globally reachable special-purpose assignments), multicast and IPv6; prefer already-configured public IPv4 resolvers but fall back to `1.1.1.1`/`8.8.8.8`; stop model-facing services before network reconciliation and fail before restarting them if the boundary cannot be established; treat policy drift during operator exec as a repair-required failure rather than silently rebuilding it.
- **Plan updates:** none. The canonical spec/sweep/plan already selected the trusted-root namespace/veth/nftables model and the implementation matched it.
- **Known defects/risks:** no cloud Phase 2 defect is known. The cloud runner cannot execute Apple Container or the guest nftables topology, so runtime syntax/kernel/systemd/provider integration and physical host/private-LAN isolation remain unaccepted until the explicit Phase 7 checklist runs on the target Mac. The threat model remains the accepted one: trusted guest kernel/root control plane versus an unprivileged coding agent, not hostile guest root/kernel containment.
- **Next handoff:** begin Phase 3 at `Secure MCP ingress and durable TTL start/stop lifecycle`. Read the Phase 3 plan section plus the completed Local setup/network/state boundary first; implement tunnel credential isolation, `local start --ttl`, `local stop`, durable lease generations and a host-side expiry supervisor with injected provider/tunnel/clock/process failure tests; do not wait for live Apple/OpenAI access before continuing through the cloud phases.

### 2026-08-13 — Phase 2 — `in_progress`

- **Agent/session:** subsequent implementation agent on the Zodex Sprite checkout.
- **Starting state:** fetched and fast-forwarded clean `main` to `d17bfe2d0882cccbf94a65f019b54930be001ba2`, incorporating the Mac agent's completed Phase 1 live evidence.
- **Ending commit(s):** Phase 2 foundation commit `7028bcaa48705a045dfc1b8e47af68a55c27c1a1`.
- **Outcome:** implemented a coherent, non-user-visible Phase 2 foundation while keeping the security gate fail closed. Added an embedded Ubuntu 24.04/systemd machine recipe, deterministic no-home-mount create/resource reconciliation, root `container machine run` command construction that preserves argument tokens, stdin-based bounded file transfer, durable nonsecret setup-source references, runtime-installer-based guest provisioning, loopback-only Zodex daemon configuration, explicit `zodex-agent` and `zodex-publisher` systemd identities, a separate restricted `zodex-tunnel` identity/directories, post-provision verification logic and setup/reconcile classification that refuses unmanaged machine adoption. `zodex local setup` and `zodex local exec` remain unregistered until the cloud Phase 2 implementation replaces the placeholder network gate and its deterministic fail-closed contracts pass.
- **Files/areas changed:** `src/bin/zodex/local_provider.rs`, `local_state.rs`, new `local_setup.rs`, embedded `local_machine.Containerfile`, provider-private `local_zodexd.service`/`local_zodex_prd.service`, module wiring and Local unit tests.
- **Positive evidence:** `cargo test --bin zodex local_ -- --nocapture` passed 11 Local tests. New tests prove machine creation always passes `--home-mount none` and no mount/volume flags, resource reconciliation retains that invariant, operator exec keeps literal token boundaries, owned targets reconcile while unmanaged machines are rejected, the embedded guest setup uses runtime-only install/loopback binding/separate agent-publisher-tunnel identities without host PEM paths, conservative setup input validation, fail-closed ready-state source requirements and the network gate refusing progression while host isolation is unproven.
- **Regression evidence:** `cargo clippy --all-targets -- -D warnings`, the 1000-LOC source guard, Sprite registry tests, `tests/zodex_operator_cli.rs`, `tests/install_script.rs`, `tests/sprite_scripts.rs` and the full `bash scripts/check.sh` gate all passed. Existing Local help still exposes only `status`, so no incomplete setup/exec promise leaked into the CLI.
- **Deferred integration evidence:** Apple Container source at the exact 1.2.2 tag (`3c4c59cf6a1099c334f1ad6d485f79969e09f4b9`) and target-Mac probes already confirmed machines use built-in shared NAT and rejected PF as the final boundary. Phase 7 still owns first live provisioning of the namespace implementation, service membership/root separation, public/private/IPv6 probes, unprivileged bypass attempts, reboot reconciliation, `local exec`, persistence and idempotent setup.
- **Documentation:** no user docs or model-facing contracts changed in the foundation commit. The cloud continuation may wire setup/exec and their help after the complete deterministic Phase 2 boundary passes; production readiness remains gated by Phase 7.
- **Decisions made:** preserve the fail-closed gate before setup can become ready. Replace its mechanism with one root-owned Linux network namespace containing daemon, publisher, tunnel and descendants; give it only loopback plus a dedicated veth; drop root-namespace input from that veth; allow/NAT only public IPv4 forwarding with interface-scoped nftables; deny all IPv6/non-public IPv4; use public IPv4 DNS; keep provider-root `local exec` outside the namespace; trust the guest kernel/root control plane while denying the unprivileged agent sudo and network/namespace capabilities. Preserve PEMs as operator source-path references only and transfer contents over provider stdin into private guest temp files.
- **Plan updates:** the namespace decision is folded directly into the canonical specification, sweep, implementation plan, progress handoff and continuation prompt. No amendment overlay is used.
- **Known defects/risks:** Phase 2 is not complete and no public setup/exec command exists yet. The namespace/veth/nftables lifecycle, systemd attachment/order, public DNS, restart reconciliation and unprivileged bypass contracts still require cloud implementation. Their real behavior remains intentionally unaccepted until Phase 7 runs and repairs them on `zodex-local`.
- **Next handoff:** continue Phase 2 in a cloud checkout at `7028bcaa48705a045dfc1b8e47af68a55c27c1a1` or newer `main`. Read the current Local provider/setup/state/service foundation, implement one root-owned agent-network responsibility in place of `local_host_network_isolation_gate`, attach every model-facing service to it, run the full deterministic Phase 2 positive/failure/regression contract, wire `local setup`/`local exec`, record the deferred Phase 7 checklist, and then continue to Phase 3 without waiting for Apple-machine access.

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
