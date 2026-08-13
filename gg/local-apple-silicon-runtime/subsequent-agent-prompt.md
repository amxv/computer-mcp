# Cloud Implementation Agent Prompt - Local Apple Silicon Runtime

Continue the Zodex Local Apple Silicon Runtime workstream from the earliest incomplete cloud phase. Phase 1 is immutable and complete. Phase 2 is in progress, and its hidden foundation is already committed at `7028bcaa48705a045dfc1b8e47af68a55c27c1a1`.

Phases 2 through 6 are designed for cloud implementation without access to the target Apple Silicon Mac. Complete as many contiguous cloud phases as this session can finish with deterministic evidence. Do not begin Phase 7 in a cloud environment. Phase 7 is the later Apple Silicon integration, live repair and final acceptance gate.

## Repository and branch

- Repository: `amxv/zodex`
- Approved branch: `main`
- Workstream folder: `gg/local-apple-silicon-runtime/`

Read the current checkout's `AGENTS.md` in full. Inspect the worktree, fetch first and fast-forward safely where possible. Preserve newer remote/local work and unrelated edits. Never force-push, rewrite shared history, hard reset away another agent's work, clean another agent's files or stage unrelated changes. Current source, tests, generated contracts and docs are truth; ledger entries and commit coordinates are evidence to verify.

## Determine the current cloud phase

Read in this order:

1. `local-apple-silicon-runtime-progress.md`: Phase table, Current handoff and entries relevant to the earliest incomplete phase.
2. `local-apple-silicon-runtime-implementation-plan-2026-08-13.md`: Delivery model, Execution protocol, Decisions and Assumptions, Acceptance Criteria and the earliest incomplete phase in full.
3. `local-apple-silicon-runtime-spec-2026-08-13.md` when the phase touches a hard product/security boundary.
4. Only the sweep sections and landmines named by the current phase.
5. Every current source/test/asset named under the phase's `Files to read before starting`.

Do not use `first-agent-prompt.md`; it is retired Phase 1 material. If modules moved, find and inspect the current symbols instead of treating old paths as commands.

## Cloud completion model

For Phases 2 through 6, missing Apple Container, Secure MCP Tunnel credentials, ChatGPT connectivity or an authorized GitHub write target does not block completion. Each cloud phase must still provide:

- its complete implementation boundary, not a placeholder;
- deterministic positive tests for generated commands/assets/state transitions;
- injected provider, filesystem, service, clock and failure-path coverage appropriate to the phase;
- relevant Sprite/MCP/GitHub/install/package regressions;
- public help/docs/generated contracts at the boundary required by that phase;
- an exact deferred Phase 7 integration checklist;
- `bash scripts/check.sh` and any additional phase checks passing;
- a coherent pushed handoff and updated ledger.

Do not fake live evidence. Record it as deferred by design and continue to the next cloud phase once the current deterministic contract is complete. A cloud-complete phase is implemented, but the product is not live-accepted or release-ready.

## Current Phase 2 foundation

The committed foundation already provides public read-only `local status`, provider capability/inspect parsing, embedded Ubuntu/systemd image assets, explicit no-home-mount create/reconcile, literal-argv provider-root exec, bounded stdin transfer, durable nonsecret Local setup/state records, runtime guest provisioning, loopback daemon configuration, explicit agent/publisher/tunnel identities, post-provision verification and a fail-closed placeholder `local_host_network_isolation_gate`.

Extend those responsibilities. Do not rebuild them as a second Local provider. Replace the placeholder with one focused root-owned Linux agent-network responsibility, finish deterministic setup/exec contracts and wire the public commands. Real Apple-machine execution and repair are deferred to Phase 7.

## Approved architecture

- Keep Apple Container as the persistent VM/performance provider.
- Trust the Local Linux kernel and root control plane in V1.
- Put `zodexd`, `zodex-prd`, `zodex-tunnel` and every descendant in one root-owned network namespace.
- Give it only loopback and a dedicated veth, never the Apple provider interface.
- Use root-owned interface-scoped nftables: drop root-namespace input from the veth, allow/NAT only public IPv4 forwarding, deny non-public IPv4 and all IPv6, and use public IPv4 DNS.
- Give `zodex-agent` no sudo, `CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, namespace control or writable policy/service assets.
- Keep `zodex local exec` in the trusted root namespace and never reuse it for MCP execution.
- Fail setup/start/readiness closed on topology, policy or service-membership drift.
- Do not implement per-address PF, whole-bridge PF, a model-facing provider NIC or another VM provider.
- Do not claim containment after trusted guest-root or Linux-kernel compromise.

Other fixed boundaries remain: no macOS mounts or ambient credentials/services; separate agent/publisher/tunnel identities; exact three-tool MCP parity with no target field; independent Local access and GitHub authority; reset as the explicit destructive operation; Sprite as a simultaneous first-class target; and fail-closed target ambiguity.

## Phase execution

Treat the current phase's files, work, validation and non-regression sections as its contract. Keep provider/systemd/nftables/tunnel behavior behind narrow injectable seams so Phase 7 failures are localizable. Keep modules focused and within the source-size guard. Re-check unstable primary-source provider/tunnel contracts when they become load-bearing, but do not substitute documentation review for the deferred live tests.

When a cloud phase is complete, update its row and Current handoff, write the required progress entry, commit/push a coherent slice, verify remote equality and continue to the next phase if capacity allows. Stop after Phase 6 and hand off to the local Phase 7 prompt/contract.

If current facts disprove a load-bearing architecture assumption, stop, capture evidence and obtain any required user decision. Then rewrite affected canonical package sections directly. Do not add an amendment overlay or silently weaken the accepted goal.

Never expose or commit tokens, PEM contents, tunnel runtime keys, private host data or secret-bearing command lines. Never perform an unauthorized GitHub push.

Return a concise handoff with phases completed/in progress, full pushed SHA(s), deterministic and regression evidence, deferred integration items, known risks and the earliest remaining boundary. Do not claim the workstream complete until Phase 7 finishes on the target Apple Silicon Mac.
