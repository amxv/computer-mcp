# Local Integration Agent Prompt - Local Apple Silicon Runtime

Run Phase 7 of the Zodex Local Apple Silicon Runtime workstream on the target Apple Silicon Mac. Phases 2 through 6 should already be cloud-implemented. Your job is to integrate, diagnose, edit, retest and finally accept the implementation against the real provider and authorized external systems. This is not a read-only verification pass.

## Start from current truth

Read the checkout's `AGENTS.md`, inspect the worktree, fetch first and fast-forward safely. Preserve unrelated work. Read:

1. `local-apple-silicon-runtime-progress.md`, especially Current handoff and the completed cloud-phase entries;
2. Phase 7 and every deferred Phase 7 checklist in `local-apple-silicon-runtime-implementation-plan-2026-08-13.md`;
3. the full accepted specification;
4. the sweep's live Apple findings, landmines and current factual gaps;
5. the current implementation and deterministic tests for Local provider/setup/network/lifecycle/GitHub/reset/package behavior.

Re-check the installed Apple Container version/capabilities and current official Secure MCP Tunnel contract before mutation. Never record secret values or private host data in repository artifacts.

## Integration order

Work in this dependency order and repair each layer before continuing:

1. build/install the current operator artifact and establish provider prerequisites;
2. provision Local and prove no host mounts, exact service identities, namespace membership, root/agent separation, public IPv4 egress, private/macOS/root-namespace/IPv6 denial, persistence and idempotent setup;
3. prove operator `local exec` works in the trusted root namespace without making that authority model-facing;
4. configure the restricted tunnel credential, connect the actual Local ChatGPT app and prove start/stop/TTL/renewal/sleep/session invalidation behavior;
5. exercise `github mode --local` and Local/Sprite ambiguity using only explicitly authorized GitHub scope;
6. exercise reset preflight, destructive recreation and interrupted/stale lifecycle recovery;
7. install/test a clean packaged artifact without a source checkout and reconcile docs/help with actual behavior;
8. run a representative Rust build/persistence workflow, all 46 acceptance criteria, `bash scripts/check.sh`, package/docs checks and Sprite regressions.

Do not continue past a failed network or privilege boundary. Diagnose it, patch the owning code/assets/tests, add a deterministic regression where practical and repeat that live slice. Apply the same rule to provider parsing, systemd ordering, tunnel lifecycle, lease recovery, packaging and docs defects.

The approved V1 boundary trusts the Apple machine's Linux kernel and root control plane. Test against the unprivileged coding agent. Do not demand or claim hostile guest-root/kernel containment. Never weaken the namespace, secret, host-mount, GitHub-authority or MCP-contract boundaries to get a green result.

Use disposable/reprovisionable Local state for destructive reset checks. Never infer permission for a GitHub write; if no disposable authorized ref exists, record that exact acceptance gap without pushing elsewhere.

## Completion

Phase 7 can be complete only when the final current code passes the applicable deferred checklists and acceptance criteria, all discovered defects are fixed and retested, docs/package state matches behavior, no secret/private test material remains, the ledger contains the final evidence, changes are coherently committed/pushed, remote equality is verified and the worktree is clean apart from preserved unrelated work.

If the real platform invalidates the approved architecture rather than exposing a repairable implementation defect, stop with concrete evidence and request a user decision. After that decision, rewrite the canonical package in place instead of appending an amendment overlay.
