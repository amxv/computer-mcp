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
| 1 | Local provider/state seam and truthful read-only status | pending | — | Start by reconciling current Apple Container CLI facts with the Phase 1 provider/state reading list. |
| 2 | Persistent Local machine setup and privileged operator exec | pending | — | Requires Phase 1. Private-LAN denial is a hard live security gate; block/amend if it cannot be host-enforced. |
| 3 | Secure MCP ingress and durable TTL start/stop lifecycle | pending | — | Requires a Phase 2-ready safe Local machine and official current Secure MCP Tunnel compatibility. |
| 4 | GitHub mode target parity with `--local` | pending | — | Requires Local privileged guest transport; preserve one canonical YOLO policy and Sprite behavior. |
| 5 | Reset, reprovisioning and lifecycle recovery hardening | pending | — | Requires setup/start/target paths to exist; reset must preflight before deletion. |
| 6 | Public docs, packaging and Sprite-preserving product integration | pending | — | Document only behavior present on the current branch; preserve Sprite-first-class guidance. |
| 7 | Independent end-to-end acceptance, security and performance hardening | pending | — | Re-read full current plan/Amendments and prove all acceptance criteria from current code/live systems. |

## Current handoff

- **Last completed phase:** none.
- **Earliest incomplete phase:** Phase 1.
- **Phase title:** `Local provider/state seam and truthful read-only status`.
- **Observable boundary:** a read-only `zodex local status` plus tested Apple provider/state parsing exists without changing Sprite or MCP behavior.
- **Current blockers:** none known before implementation. Phase 2 carries the highest-regret factual uncertainty: host/provider enforcement of public-Internet egress while denying macOS/private-LAN access.
- **Plan Amendments affecting next phase:** none.
- **Prompt to use:** [`first-agent-prompt.md`](./first-agent-prompt.md).

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

None yet.
