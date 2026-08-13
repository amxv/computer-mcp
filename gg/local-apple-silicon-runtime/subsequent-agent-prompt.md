# Subsequent Implementation Agent Prompt — Local Apple Silicon Runtime

Continue the Zodex Local Apple Silicon Runtime workstream from the **earliest genuinely incomplete phase**. Finish an `in_progress` phase before moving later. After completing a phase thoroughly, continue across as many **contiguous phases** as this session can finish with their required evidence; do not stop merely because one phase completed.

## Repository and branch

- Repository: `amxv/zodex`
- Checkout: `/workspace/repos/zodex`
- Approved branch: `main`
- Workstream folder: `gg/local-apple-silicon-runtime/`

## Start from current truth

1. Read `/workspace/repos/zodex/AGENTS.md` in full.
2. Inspect `git status --short --branch`.
3. Fetch `origin` first.
4. Fast-forward `main` safely to current `origin/main` where possible.
5. Preserve newer remote/local work. Never force-push, rewrite shared history, hard reset away another agent's changes, clean another agent's files or stage unrelated work.
6. Treat current source, tests, generated/public contracts and docs as truth. Progress-ledger claims and old commits are evidence to verify, never proof.

If the current code contradicts a phase marked `complete`, reopen that acceptance gap before building later assumptions on it.

## Determine the current phase

Read:

1. `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-progress.md`
   - artifact links and repository rules;
   - Phase table;
   - `Current handoff`;
   - latest progress entries relevant to the earliest incomplete phase.
2. `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-implementation-plan-2026-08-13.md`
   - `Planning Basis`;
   - `State of Ideal System`;
   - `Decisions and Assumptions`;
   - `Acceptance Criteria`;
   - `Plan Phases > Execution protocol`;
   - current `Amendments`;
   - the **earliest incomplete phase** in full.
3. `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-spec-2026-08-13.md` when the current phase touches a hard product/security boundary or an Amendment requires reconciling intent.
4. From `local-apple-silicon-runtime-sweep-2026-08-13.md`, read **only** the sections/landmines named by the current phase.

Do not reread every later phase and do not wander through old neighboring `gg/` planning history. The point of this package is to let you load only the current boundary plus current code.

## Inspect current code for the phase

Follow the current phase's `Files to read before starting` against the branch as it exists now. Verify every named pattern/consumer/test is still real. If modules moved, find the current symbol rather than treating the planning path as an old-SHA command.

Use Git history only when it materially explains surprising current behavior, a regression, migration or compatibility constraint.

Re-check external provider/library facts at the phase where they become load-bearing:

- Apple Container/container-machine behavior for Local provider/lifecycle/network phases;
- OpenAI Secure MCP Tunnel docs/releases for ingress phases;
- actual authorized GitHub target state for live push evidence.

Do not substitute stale planning assumptions for a current provider contract.

## Cross-phase architecture that must remain stable

Unless a recorded Plan Amendment changes the mechanism, preserve these accepted boundaries:

- one persistent V1 Local Linux machine on Apple Silicon;
- no macOS home/project binds or ambient host credentials/services;
- public Internet egress but macOS/private-LAN denied by a host/provider-controlled boundary;
- model commands run as unprivileged `zodex-agent`;
- `zodex-publisher` remains separate from the agent;
- Secure MCP Tunnel authority remains separate from `zodex-agent`;
- operator privilege is out-of-band through `zodex local exec`;
- Local access is finite and explicit through `zodex local start --ttl ...`;
- stop/expiry revoke tunnel then stop machine while preserving disk;
- reset is the explicit destructive workspace-wipe operation and preflights reprovisioning inputs first;
- GitHub YOLO remains independent and uses `--local` rather than Local start granting push;
- Sprite remains a simultaneous first-class target;
- Local and Sprite have separate MCP app identities;
- the model-facing MCP surface remains exactly `exec_command`, `write_stdin`, `apply_patch` with no provider/target field and unchanged annotations;
- target ambiguity fails closed;
- avoid a public generic `zodex target` abstraction or duplicated GitHub policy implementations.

### Critical network rule

The plan deliberately treats private-LAN denial as a hard gate. If live Apple behavior cannot enforce **public Internet allowed + macOS/private LAN denied** at a host/provider-controlled boundary, do not quietly fall back to a guest firewall and continue. Mark the phase blocked, capture evidence, and add an Amendment choosing a viable host-enforced architecture or escalating the conflict. The user's security goal is not negotiable because implementation is inconvenient.

## Implement the current phase completely

Use the phase's four subsections as the contract:

- `Files to read before starting`
- `What to do`
- `Validation strategy`
- `What must not break`

Preserve judgment/constraints, but adapt local helper/file choices to current code. Do not follow the plan as stale pseudocode.

Keep each phase vertically exercisable. Avoid unrelated cleanup. If a class-level refactor is required by the feature (for example shared operator guest transport for GitHub mode), end with one canonical path and do not leave a permanent old/new duplicate.

## Verification expectations

A phase is not complete because it compiles or because a regression suite that passed before still passes.

For every phase:

1. Produce **positive evidence** whose result changes because of the phase.
2. Produce **regression evidence** for the load-bearing old behavior named by that phase.
3. Use **real integration evidence** where the plan requires it and the authorized environment exists.
4. Use deterministic injected-failure/race/time/provider fixtures for states that live testing cannot reliably induce.
5. Update CLI help/docs/generated public contracts at coherent user-visible boundaries, never ahead of shipped behavior.
6. Use the repo's real validation stack and canonical commands; `bash scripts/check.sh` is the broad gate.

Do not fake Apple/Tunnel/GitHub evidence when the required live system or authorization is unavailable. Record the exact gap and let the phase/acceptance contract determine whether completion is blocked.

Never expose or commit GitHub tokens/PEMs, OpenAI tunnel runtime keys, private host data, local databases, raw sensitive live content or secret-bearing command lines.

## Amendments

The implementation plan is living execution guidance.

When current reality disproves a load-bearing assumption/approach:

1. add the Amendment **when the decision is made**;
2. include date and phase;
3. state what the plan assumed/instructed;
4. state current evidence proving it wrong;
5. state the selected replacement and why;
6. state exactly which later phases are affected.

An Amendment can change implementation technique. It cannot quietly weaken the accepted product/security scope.

Routine local helper choices that do not alter later-agent guidance belong in the progress entry, not Amendments.

## Progress and commit discipline

After each phase completed or blocked in this session:

1. update its row in `local-apple-silicon-runtime-progress.md` immediately;
2. update `Current handoff` to the earliest genuinely incomplete phase;
3. append the complete required progress entry with actual evidence;
4. commit a coherent safe slice;
5. push normally to `main` when repository policy allows;
6. if remote advanced, integrate it safely rather than overwriting it;
7. fetch after push and verify local full SHA equals `origin/main`;
8. verify a clean worktree except for any pre-existing preserved work belonging to another agent.

If this session ends before the current phase is complete:

- leave it `in_progress`;
- push only a coherent branch-safe slice;
- record exact finished/unfinished boundaries, tests actually run and next action;
- do not jump to a later phase.

## Final response

Return concise but exact handoff coordinates:

- phase(s) completed/blocked/in-progress;
- full pushed SHA(s);
- positive, regression and live evidence actually obtained;
- docs/generated-contract validation;
- Amendments made, if any;
- known remaining defects/risks;
- earliest remaining phase and exact boundary.

Do not claim the workstream complete until Phase 7 independently maps the final current implementation to every acceptance criterion and finishes its broad/live acceptance contract.
