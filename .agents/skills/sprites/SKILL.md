---
name: sprites
description: Use when operating Sprites from the CLI, including auth/context setup, command execution, networking, persistence behavior, checkpoints, day-to-day workflows with an existing Sprite, or diagnosing access differences between the default sprite guest user and the separate zodex-agent account used by coding agents.
---

# Sprites

Supported framing for this repo:
- use `zodex` for operator-facing commands and product naming
- use Sprite-first language and workflows throughout

Use this skill when work involves understanding or operating Sprites with the `sprite` CLI.

This skill is based on:

- https://docs.sprites.dev/
- https://docs.sprites.dev/quickstart/
- https://docs.sprites.dev/working-with-sprites/
- https://docs.sprites.dev/cli/commands/
- https://docs.sprites.dev/cli/authentication/
- https://docs.sprites.dev/cli/installation/

## Core Mental Model

A Sprite is a persistent cloud Linux environment.

- Filesystem persists across sleep/wake
- Processes and RAM do not persist when a Sprite sleeps
- Sprite wakes automatically on CLI command execution and HTTP requests
- Each Sprite has a URL and supports local-to-remote port forwarding

Treat it like a remote dev box with automatic hibernation.

## Guest identities and agent access

`sprite exec` runs non-interactive commands as the `sprite` Linux user by
default. On the zodex Sprite this is UID 1001 with home `/home/sprite`; it is
not root. The `sprite` user may have passwordless sudo for operator tasks, but
commands run as `sprite` do not prove what a coding agent can access.

Coding agents use the separate `zodex-agent` Linux account. On the zodex
Sprite this is UID 997 with home `/home/zodex-agent` and its own `PATH`, local
installations, files, and configuration. For example, AgentBox is resolved
from `/home/zodex-agent/.local/bin/agentbox` and its profile is read from
`/home/zodex-agent/.config/agentbox/profiles.json`.

When checking agent-visible tools, credentials, or configuration, run the
command explicitly as `zodex-agent`:

```bash
sprite exec -s <sprite> -- sudo -n -iu zodex-agent -- /bin/bash -lc \
  'id; whoami; printf "HOME=%s\\n" "$HOME"; command -v <command>; <command>'
```

Check both identities when diagnosing a discrepancy. A successful command as
the default `sprite` user is not evidence that the same command succeeds for
`zodex-agent`.

Repository permissions can differ even when the default user can list a
workspace. On the zodex Sprite, `/workspace/repos/zodex` and its Git metadata
are owned by `zodex-agent:zodex`; `sprite` can see the worktree but cannot read
`.git/config`. Git may therefore report `not a git repository` when run as
`sprite`. Run Git checks as the agent account:

```bash
sprite exec -s <sprite> -- sudo -n -u zodex-agent -H \
  git -C /workspace/repos/zodex status --short --branch
```

## CLI Command Groups

Use commands in four groups:

1. Auth and context
- `sprite org auth`
- `sprite org list`
- `sprite use <sprite-name>`
- `sprite list`

2. Execution and sessions
- `sprite exec ...` for one-off commands and scripts
- `sprite console` for interactive shell work
- `sprite sessions list|attach|kill` for detached TTY sessions

3. Networking
- `sprite url` and `sprite url update --auth <sprite|public>`
- `sprite proxy <port>` or `<local:remote>` for TCP forwarding

4. State safety and lifecycle
- `sprite checkpoint create|list|restore`
- `sprite destroy`

## Recommended Operator Workflow (Existing Sprite)

For an existing Sprite (example: `coding-sprite`):

```bash
sprite use coding-sprite
sprite exec 'echo hello && uname -a && pwd'
sprite console
```

Use `sprite exec` for automation and repeatable scripts.
Use `sprite console` for exploratory debugging.

For `zodex` on Sprites, prefer control-plane lifecycle commands over in-guest service management:

- Initial install or full reconfiguration: `zodex sprite setup --sprite <sprite> --repo <owner/repo> ...`
- Routine upgrade: `zodex sprite upgrade --sprite <sprite> [--org <org>]`
- If control-plane state is stale: `zodex sprite sync --sprite <sprite> [--org <org>] --force-recreate`

That keeps Sprite Services as the lifecycle owner and avoids depending on guest-local process state when upgrading.

## Persistence Rules

Persists:
- Installed packages
- Files, git repos, and DB files on disk
- Network and URL settings

Does not persist through sleep:
- Running ad hoc processes from interactive sessions
- In-memory state

If a service must survive hibernation/wake cycles, run it as a managed Service (for example with `sprite-env services ...`) so it can restart automatically.

## Networking and URL Auth

```bash
sprite url
sprite url update --auth public
sprite url update --auth sprite
```

- Private/authenticated URL mode is safer for normal development
- Public mode is appropriate for demos, webhooks, or intentionally public endpoints
- Never expose secrets in publicly reachable handlers

Port forwarding examples:

```bash
sprite proxy 5432
sprite proxy 3001:3000
sprite proxy 3000 8080 5432
```

## Checkpoint Discipline

Use checkpoints before risky changes:

```bash
sprite checkpoint create --comment "before dependency upgrade"
sprite checkpoint list
sprite restore <version-id>
```

Restoring replaces the full filesystem state with the checkpoint version.

## Practical One-Liners

```bash
sprite exec --dir /home/sprite/project npm test
sprite exec --env NODE_ENV=production,DEBUG=1 node app.js
sprite exec --file ./local.env:/home/sprite/.env cat /home/sprite/.env
```

## Security and Hygiene

- Keep `.sprite` out of git (`.gitignore`)
- Use organization-scoped auth and tokens
- Use private URL auth mode by default
- Prefer checkpoints before destructive experiments
- Treat `sprite destroy` as irreversible

## Version Drift Rule

Docs can differ slightly across pages and CLI versions.
When behavior is ambiguous, trust the installed CLI help first:

```bash
sprite --help
sprite <subcommand> --help
sprite url update --help
```

## Quick Troubleshooting

```bash
sprite org auth
sprite list
sprite exec ps aux
sprite exec df -h
sprite exec free -h
```

If auth is broken, refresh Fly.io auth and retry `sprite org auth`.
