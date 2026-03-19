# Offline Agent Git Bundle Workflow

## Problem This Is Trying To Solve

We want a fallback workflow for agents running in very restricted ChatGPT container environments.

The constrained container we inspected is good at local file editing and Git operations, but it is not a good fit for arbitrary outbound network workflows.

From the container report, the important constraints are:

- it has `git`, `zip`, `unzip`, `tar`, `bash`, `python`, `node`, `go`, `gcc`, and a local `apply_patch`
- it does **not** have normal outbound internet access
- network access is limited to package-manager proxy infrastructure
- it does **not** have Rust toolchain binaries like `cargo` or `rustc`

So the container is good at:

- unpacking files
- editing local repos
- making Git commits
- producing artifacts like zip files and git bundles

But it is not good at:

- directly connecting to the live VPS with arbitrary binaries
- relying on outbound DNS/network access to remote hosts
- running Rust validation unless a toolchain is separately provided

That means the best workflow for these agents is not "connect to the VPS and code remotely."

The best workflow is:

1. give the agent a full local repo snapshot as an offline task pack
2. let the agent edit the repo locally with its built-in `apply_patch`
3. have the agent commit its changes locally
4. have the agent return a `git bundle` inside a zip file
5. import that bundle into the real repo and review/apply it

This gives us a Git-native offline handoff path that does not require live network access.

## Design Summary

The workflow should use two artifacts:

1. an **input artifact** we create and upload to the agent
2. an **output artifact** the agent returns to us

The output artifact should be a real Git handoff, not raw diff text, because Git bundles are much easier to verify, inspect, import, and apply cleanly.

## Why This Design Fits The Container

This design matches the capabilities of the restricted container very well:

- the agent already has a local `apply_patch`
- the agent has `git`
- the agent has `zip` / `unzip`
- the agent can work completely offline after the input artifact is uploaded

The only meaningful limitation is validation:

- for Rust repos, the agent may not be able to run `cargo test` or `cargo build`
- in that case it should still make the change and clearly report what validation could not be run

So the initial version of this workflow should optimize for:

- offline editing
- offline commit production
- bundle-based handoff

not for:

- full toolchain validation inside the restricted container

## Input Artifact: Offline Task Pack

The input artifact should be a zip file containing a full repo snapshot plus enough metadata for the agent to work safely.

Recommended contents:

- `repo/`
- `repo/.git/`
- `TASK.md`
- `BASE_COMMIT`
- optionally `VALIDATION.md`

### `repo/`

This should be a full working tree with `.git` included. The `.git` directory must be present so the agent can:

- inspect history
- create a branch
- commit locally
- produce a bundle

### `TASK.md`

This should describe exactly what the agent is supposed to change.

It should include:

- the requested feature or fix
- any constraints
- any files or areas to avoid
- any validation expectations

### `BASE_COMMIT`

This should contain the exact commit SHA that the agent must build on.

This is important because:

- the agent should branch from an explicit base
- the returned bundle should be generated relative to that base
- we want a deterministic import path on the receiving side

### `VALIDATION.md` (optional)

This can describe what the agent should try to run locally and what is acceptable to skip if the toolchain is unavailable.

For example, for a Rust repo:

- try `cargo test` if available
- otherwise record that Rust validation could not be run in this container

## How To Create The Input Artifact

This is the recommended manual process from the source repo:

```bash
BASE_SHA=$(git rev-parse HEAD)

rm -rf /tmp/offline-task-pack
mkdir -p /tmp/offline-task-pack

git clone --no-hardlinks . /tmp/offline-task-pack/repo
git -C /tmp/offline-task-pack/repo checkout "$BASE_SHA"
git -C /tmp/offline-task-pack/repo clean -fdx

printf '%s\n' "$BASE_SHA" > /tmp/offline-task-pack/BASE_COMMIT
cp TASK.md /tmp/offline-task-pack/TASK.md

cd /tmp
zip -r offline-task-pack.zip offline-task-pack
```

Important notes:

- use command-line `zip -r`, not a GUI zipper, so `.git` is definitely included
- start from a clean source repo
- package an exact base commit, not a moving branch tip with stray local changes

## Output Artifact: Agent Handoff Zip

The output artifact the agent returns should also be a zip, but the actual handoff content inside it should be Git-native.

Recommended contents:

- `handoff/changes.bundle`
- `handoff/BASE_COMMIT`
- `handoff/HEAD_COMMIT`
- `handoff/BRANCH`
- `handoff/SUMMARY.md`
- `handoff/DIFFSTAT.txt`
- `handoff/COMMITS.txt`

### `changes.bundle`

This is the real payload.

It should contain the commits the agent made on top of the provided base commit.

### `BASE_COMMIT`

This should match the input base commit.

### `HEAD_COMMIT`

This should contain the final commit SHA produced by the agent.

### `BRANCH`

This should contain the branch name the agent used.

### `SUMMARY.md`

This should explain:

- what changed
- what validation was run
- what validation could not be run
- any important notes or caveats

### `DIFFSTAT.txt`

This gives a quick review summary of touched files.

### `COMMITS.txt`

This gives a quick list of commit messages included in the bundle.

## Instructions To Give The Offline Agent

This is the high-level instruction set the agent should receive:

```text
You are working fully offline in a restricted container.

You do have:
- git
- zip/unzip
- bash
- local apply_patch

You do not have normal internet access, so do not try to call remote services.
Your task is to make the requested code change locally and hand back a git bundle.

Files provided:
- `repo/` — full repo checkout with `.git`
- `TASK.md` — requested change
- `BASE_COMMIT` — the exact base commit you must build on

Instructions:

1. Read `TASK.md`.
2. Read `BASE_COMMIT`.
3. `cd repo`
4. Configure git identity if needed:
   - `git config user.name "OpenAI Agent"`
   - `git config user.email "agent@local.invalid"`
5. Create a branch from the base commit:
   - `git switch -c agent/task-work $(cat ../BASE_COMMIT)`
6. Make the requested changes using your local apply_patch tool.
7. Run any local validation you can with the tools available in this container.
   If the required toolchain is missing, say so clearly in your summary.
8. Commit the changes locally.
9. Create a `handoff/` directory containing:
   - `changes.bundle`
   - `BASE_COMMIT`
   - `HEAD_COMMIT`
   - `BRANCH`
   - `SUMMARY.md`
   - `DIFFSTAT.txt`
10. Zip `handoff/` and return `handoff.zip`.

Important rules:
- do not rebase
- do not rewrite history
- do not return only raw diff text if you can return the bundle artifact
- the bundle must contain committed changes only
```

## Exact Commands The Agent Should Run

These commands are compatible with the restricted container we inspected.

### Set up the local branch

```bash
cd repo
BASE_SHA=$(cat ../BASE_COMMIT)

git config user.name "OpenAI Agent"
git config user.email "agent@local.invalid"
git switch -c agent/task-work "$BASE_SHA"
```

### Make edits

Use the container's local `apply_patch` to make the requested change.

### Commit locally

```bash
git add -A
git commit -m "Implement requested change"
```

### Create the handoff directory and bundle

```bash
BRANCH=$(git branch --show-current)
mkdir -p ../handoff

git bundle create ../handoff/changes.bundle "refs/heads/$BRANCH" "^$BASE_SHA"
git bundle verify ../handoff/changes.bundle

printf '%s\n' "$BASE_SHA" > ../handoff/BASE_COMMIT
git rev-parse HEAD > ../handoff/HEAD_COMMIT
printf '%s\n' "$BRANCH" > ../handoff/BRANCH
git diff --stat "$BASE_SHA"..HEAD > ../handoff/DIFFSTAT.txt
git log --oneline --decorate "$BASE_SHA"..HEAD > ../handoff/COMMITS.txt
```

### Write the summary

```bash
cat > ../handoff/SUMMARY.md <<'EOF'
What changed:
- ...

Validation run:
- ...

Validation not run:
- ...

Notes:
- ...
EOF
```

### Zip the output artifact

```bash
cd ..
zip -r handoff.zip handoff
```

## How We Plan To Apply The Returned Bundle

Once the agent sends back `handoff.zip`, the receiving side should:

1. unzip it
2. verify the bundle
3. fetch the bundle into the real repo as a temporary review ref
4. inspect the commits and diff
5. cherry-pick or otherwise apply the commits

### Unzip and inspect metadata

```bash
unzip handoff.zip -d /tmp/agent-handoff
cat /tmp/agent-handoff/handoff/BASE_COMMIT
cat /tmp/agent-handoff/handoff/BRANCH
```

### Verify and fetch the bundle

```bash
git bundle verify /tmp/agent-handoff/handoff/changes.bundle
git fetch /tmp/agent-handoff/handoff/changes.bundle \
  "refs/heads/$(cat /tmp/agent-handoff/handoff/BRANCH):refs/remotes/offline/$(cat /tmp/agent-handoff/handoff/BRANCH)"
```

### Review the imported branch

```bash
OFFLINE_REF="refs/remotes/offline/$(cat /tmp/agent-handoff/handoff/BRANCH)"
BASE_SHA=$(cat /tmp/agent-handoff/handoff/BASE_COMMIT)

git log --oneline --decorate "$BASE_SHA"..$OFFLINE_REF
git diff --stat "$BASE_SHA"..$OFFLINE_REF
```

### Apply the commits

If we want to preserve the exact commits:

```bash
git cherry-pick "$BASE_SHA"..$OFFLINE_REF
```

If we want to open a review branch first:

```bash
git switch -c review/offline-bundle $OFFLINE_REF
```

## Important Workflow Rules

### The agent must commit before bundling

This is critical.

A git bundle only contains committed history. Uncommitted working tree changes are not included.

### Do not rely on raw diff text

Raw diffs are harder to verify and import cleanly.

The preferred artifact is always:

- committed Git history
- packaged as a bundle

### Do not assume validation is available

For some repos, especially Rust repos, the container may not have the required toolchain.

That means the workflow should tolerate:

- code change completed
- commit completed
- bundle created
- validation partially skipped with a clear explanation

### Start with edit-only mode

For the first version of this workflow, the simplest and most realistic mode is:

- agent edits locally
- agent commits locally
- agent returns bundle
- validation is best-effort

This is better than over-designing around remote execution that the container cannot reliably perform.

## Scripts We Plan To Write

To make this workflow ergonomic, we should add two helper scripts to this repo.

### `scripts/make_offline_agent_pack.sh`

Purpose:

- create the offline input artifact from the current repo

Expected responsibilities:

- verify the repo is in a suitable state for packaging
- capture the current base commit
- clone/export the repo with `.git`
- include `TASK.md` and `BASE_COMMIT`
- produce a single zip file ready to upload to the offline agent

### `scripts/import_offline_agent_bundle.sh`

Purpose:

- import and verify the agent's returned bundle

Expected responsibilities:

- unzip the returned handoff artifact
- verify the bundle
- fetch it into a temporary review ref
- print:
  - imported branch name
  - base commit
  - head commit
  - commit list
  - diffstat
- optionally offer a safe cherry-pick path

## Recommended Initial Rollout

The best first implementation path is:

1. document this workflow
2. write the two helper scripts
3. test the workflow with a small repo and a trivial change
4. only then expand into richer ergonomics if needed

The point of this design is not to simulate the full live VPS experience.

The point is to give restricted agents a reliable offline way to:

- receive a real repo
- make a real Git commit
- return a real Git-native handoff artifact

That is the right backup path for locked-down ChatGPT containers.
