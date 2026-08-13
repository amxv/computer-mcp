# First Implementation Agent Prompt — Local Apple Silicon Runtime

Work on **Phase 1 only** of the Zodex Local Apple Silicon Runtime workstream.

## Repository and branch

- Repository: `amxv/zodex`
- Checkout: `/workspace/repos/zodex`
- Approved branch: `main`
- Workstream folder: `gg/local-apple-silicon-runtime/`

Do not create a feature branch unless the user explicitly changes the branch policy. Do not implement later phases in this run.

## Start safely

1. Read `/workspace/repos/zodex/AGENTS.md` in full.
2. Inspect `git status --short --branch` before touching anything.
3. Fetch `origin` first.
4. Fast-forward `main` safely to current `origin/main` if possible.
5. If remote work advanced since planning, preserve and understand it. Current remote source/tests/docs are authoritative; the planning SHA is only provenance.
6. Never force-push, rewrite shared history, hard reset away work, clean another agent's files, or stage unrelated changes.
7. If another agent has uncommitted work, leave it untouched and constrain your changes/staging around it.

## Planning material to read

Read these orientation artifacts before implementation:

1. `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-spec-2026-08-13.md` — read in full; this is the accepted product/security contract.
2. `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-implementation-plan-2026-08-13.md` — read:
   - `Planning Basis`
   - `State of Current System`
   - `State of Ideal System`
   - `Decisions and Assumptions`
   - `Acceptance Criteria`
   - `Plan Phases > Execution protocol`
   - **Phase 1 only**
   - current `Amendments`
3. `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-progress.md` — read the artifact links, repository/ledger rules, Phase table, Current handoff and latest progress entries.
4. From `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-sweep-2026-08-13.md`, read only the sections named by Phase 1:
   - `Operator CLI module shape`
   - `Persistence and identity`
   - `Public API / CLI / UI / schema / generated contracts`
   - Landmines 2, 6, 13, 14, 18

Do **not** wander through old neighboring `gg/` plans or historical planning Markdown. The current code is the implementation truth.

## Inspect the exact current Phase 1 code

Follow Phase 1's `Files to read before starting` against the current checkout. Re-open the real current symbols rather than assuming they are unchanged from planning:

- `src/bin/zodex/prelude.rs` — current command/state declarations.
- `src/bin/zodex/dispatch.rs` — current command routing.
- `src/bin/zodex/credentials.rs` — Sprite registry resolution and TTL parsing.
- `src/bin/zodex/sprite_proxy.rs` — Sprite provider exec pattern only; do not generalize Sprite lifecycle unnecessarily.
- `tests/zodex_operator_cli.rs`.
- `src/bin/zodex/tests/part2.rs` around Sprite registry and TTL tests.
- `tests/source_file_size.rs`.
- `scripts/install.sh` around macOS operator detection/install.
- `.github/workflows/release.yml`.

Re-check the current official Apple Container `container machine` documentation/installed CLI before locking command shapes. This is a current provider capability, not a fact to trust forever from the plan.

## Implement Phase 1 only

The observable Phase 1 outcome is:

> Zodex has a clean Local provider/state seam and a truthful **read-only** `zodex local status` surface that can classify unsupported/missing provider, not-configured and observed machine state without changing Sprite or MCP behavior.

Preserve the cross-phase names/ownership from the plan where they are relevant now:

- one V1 Local target;
- `LocalTargetRecord` for durable nonsecret setup metadata;
- `LocalAccessLease` for later access-window state;
- Apple-specific Local provider code isolated by responsibility;
- no user-facing generic `zodex target` abstraction;
- no raw secrets in Local host records.

Phase 1 should establish the minimum future-proof state/provider foundation, not implement Local setup/start/tunnel/GitHub behavior prematurely.

In particular:

- add `zodex local` and a truthful read-only `status` path;
- establish Apple Silicon/macOS/container-machine capability detection and deterministic provider command/parsing seams that still compile/test on Linux;
- add atomic/user-private Local state loading/saving and incomplete-vs-ready representation only as needed by the plan;
- reuse/generalize the existing Zodex duration grammar instead of creating a new `--ttl` parser;
- keep `zodex sprite ...` and the Sprite registry unchanged;
- keep the model-facing MCP tool schemas/annotations unchanged;
- do not partially ship later Local subcommands with behavior they cannot yet fulfill;
- do not store API keys, PEM contents or tunnel key contents in Local state.

If current code shows that the planned module/state boundary is wrong in a load-bearing way, add a dated Phase 1 Amendment to the implementation plan **before** implementing the divergent path. An Amendment can change the mechanism, not the accepted isolation/authority/product goal.

## Evidence required before Phase 1 can be complete

Do not mark Phase 1 complete merely because code compiles or the old suite passes.

Produce the Phase 1 positive evidence named in the plan, including tests that specifically become true because of this phase:

- Local status/help behavior;
- provider capability/status classification from deterministic fixtures;
- Local state safety/atomicity as implemented;
- shared TTL grammar including `2d`;
- runtime-gated unsupported-platform behavior rather than making the shared CLI Darwin-only.

Also run regression evidence for the existing Sprite target resolver/registry and GitHub mode help/TTL behavior.

Use the repository's current validation commands rather than inventing replacements. The canonical broad gate is `bash scripts/check.sh`, but scope validation sensibly during iteration and run the level required by the phase/changed code before completion.

### Live provider evidence

On an actual supported Apple Silicon Mac, Phase 1 calls for a **read-only** `zodex local status` capability/provider probe against the installed Apple Container version. If this execution session does not have access to such a Mac, do not fake it and do not claim the live evidence passed. Record the concrete unavailable evidence in the ledger and determine from the plan whether that prevents Phase 1 from being complete. A provider capability assertion that is architecturally load-bearing must eventually be proven live.

Do not perform destructive provider/GitHub actions without explicit authorization.

## Documentation/contracts

Update CLI help/tests that are part of the new Phase 1 public `local status` surface. Do not write user docs for `setup`, `start`, tunnel access or GitHub `--local` yet because those capabilities do not exist in Phase 1.

Do not change MCP schemas/generated model-facing contracts.

## Finish and hand off durably

Before ending:

1. Update the Phase 1 row in `local-apple-silicon-runtime-progress.md`.
2. Update `Current handoff` to the earliest genuinely incomplete phase/boundary.
3. Append the full required progress entry with actual positive/regression/live/docs evidence and known gaps.
4. Add a plan Amendment immediately if one was required.
5. Make a coherent commit containing the Phase 1 implementation, tests, applicable docs/help and progress updates.
6. Push normally to `main`.
7. Fetch `origin` after push.
8. Verify the full local `HEAD` SHA equals `origin/main`.
9. Verify the working tree is clean except for any pre-existing preserved work belonging to another agent.

Return:

- full pushed SHA(s);
- Phase 1 observable outcome;
- exact positive/regression/live validation performed;
- any Amendment(s);
- any remaining known risk/gap;
- exact Phase 2 handoff boundary.

Do not start Phase 2 in this first-agent run.
