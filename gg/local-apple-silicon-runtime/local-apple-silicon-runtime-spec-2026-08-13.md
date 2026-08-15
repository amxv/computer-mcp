# Zodex Local — Persistent Apple Silicon Execution Target

## Status

Product/architecture specification derived from the accepted design discussion on 2026-08-13.

This is a **specification and implementation target**, not a request to begin implementation yet.

## Objective

Add a new **Local** execution target to Zodex for Apple Silicon macOS while preserving the existing Sprite experience.

Zodex Local should feel to ChatGPT almost exactly like the existing Sprite-backed Zodex environment:

- same MCP tool surface
- same command/session semantics
- same `/workspace` conventions
- same unprivileged `zodex-agent` execution identity
- same persistent agent-owned Linux filesystem model
- same GitHub reader/publisher/YOLO behavior wherever possible

The important difference is infrastructure: instead of running the agent computer on Sprites.dev, Zodex Local runs a persistent isolated Linux machine on the user's Apple Silicon Mac and uses the Mac's CPU, RAM, and local storage performance. Inside that machine, model-facing services run in a root-owned restricted network namespace so they retain normal public Internet access without receiving a route to macOS or the private LAN.

The primary motivation is large local builds, especially Rust projects, where remote Sprite resources are insufficient but the user still wants the same ChatGPT-native Zodex workflow.

---

## Core product model

Treat Sprite and Local as peer execution targets:

```text
Zodex Sprite
  Remote persistent Linux agent computer
  Provider: Sprites.dev

Zodex Local
  Local persistent Linux agent computer
  Provider: Apple Silicon Mac
```

They should share the same guest/runtime implementation wherever the behavior is genuinely identical. Do not rewrite the MCP execution stack merely because Local exists.

The conceptual layering should be:

```text
                    ChatGPT
                      │
        same MCP tools / same schemas
                      │
            ┌─────────┴─────────┐
            │                   │
      Zodex Sprite         Zodex Local
            │                   │
   Sprite lifecycle       Apple-local lifecycle
   Sprite networking      Local isolated networking
   Sprite MCP ingress     Local MCP ingress/tunnel
   Sprite operator exec   Local operator exec
            │                   │
            └─────────┬─────────┘
                      │
               same guest runtime
                      │
                zodex-agent
                zodexd
                zodex-prd
                git helpers
                /workspace
```

The local implementation should be judged by how little agent-visible behavior needs to differ from Sprite.

---

## Accepted design decisions

### 1. Apple Silicon macOS is the first target

Do not optimize the first implementation for cross-platform support. Keep platform-specific code isolated enough that another provider could be added later, but prioritize a clean Apple Silicon implementation.

Apple's container/container-machine stack is the preferred implementation direction because it provides a persistent Linux environment with VM-level isolation and configurable CPU/RAM. Validate the exact APIs and lifecycle behavior during implementation rather than hard-coding this spec to a specific unstable CLI detail.

### 2. Local is a persistent Linux agent computer, not host-folder sharing

Do **not** expose arbitrary macOS folders to ChatGPT.

Do **not** mount the user's normal development folders, home directory, SSH material, Keychain data, local sockets, or other host state into the agent machine.

Zodex Local owns its own persistent Linux filesystem. Expected workspace shape:

```text
/workspace/
  repos/
    agentbox/
    zodex/
    goldengoose/
    ...
```

The normal workflow remains the same as Sprite:

```text
cd /workspace/repos/<repo>
git pull --ff-only
...work...
```

The local machine should keep useful persistent state such as:

- repositories
- `.git` data
- Rust `target/` directories
- Cargo/rustup state
- language/package caches
- installed user-space tools
- `node_modules`
- build outputs

Persistence is a feature. This is not intended to be a fresh disposable container per task or per repo.

### 3. Keep build-heavy files on the Linux machine's native persistent storage

Do not implement `/workspace` as a macOS bind mount.

For build-heavy workloads, especially Rust, repositories and build artifacts should live on the Local Zodex Linux machine's persistent block-backed/native Linux filesystem. Host-directory sharing is both an unnecessary security complication and a likely performance regression for metadata-heavy builds.

### 4. Preserve the current unprivileged agent boundary

ChatGPT/MCP commands must continue to run as the unprivileged `zodex-agent` user, matching the current Sprite design.

Do **not** give the MCP agent root or general sudo access.

This preserves the existing and well-understood authority split:

```text
zodex-agent
  owns/uses /workspace and its own home
  runs MCP commands
  can install user-space tooling
  cannot administer the Linux machine

zodex-publisher
  separate restricted identity
  retains publisher credentials/authority

operator
  can perform privileged maintenance through the local Zodex control plane
```

There is no performance benefit to running large builds as root. Root would mainly make one-time OS package setup easier while unnecessarily collapsing existing security boundaries.

### 5. Make privileged non-interactive Local exec first-class

The user wants a simple operator-only path for one-time environment setup and occasional maintenance.

Required primary UX:

```bash
zodex local exec -- sudo apt-get update
zodex local exec -- sudo apt-get install -y clang mold pkg-config
```

This command is invoked locally by the human operator and is **not** exposed to ChatGPT/MCP.

Interactive shell access may be useful later, e.g. `zodex local shell`, but it is not a priority for the first version. Non-interactive `local exec` matters much more.

The intended operational pattern is:

1. create the Local Zodex machine once
2. use `zodex local exec` to install a broad development environment (Rust, Go, Python, TypeScript/Node/Bun, common shell/build tools, libraries)
3. rarely administer it again
4. let ChatGPT work as `zodex-agent` indefinitely across recurring tasks

### 6. Local lifecycle/access UX should be simple and TTL-based

Do not reuse `mode yolo/default` language for Local machine access.

Preferred CLI surface:

```bash
zodex local setup
zodex local start --ttl 2d
zodex local stop
zodex local status
zodex local exec -- <command...>
zodex local reset
```

Semantics:

#### `zodex local setup`

Create and provision the persistent Local Zodex Linux machine and install/configure the guest Zodex runtime.

#### `zodex local start --ttl <duration>`

- start/boot the Local machine if needed
- ensure Zodex guest services are running
- make the Local MCP endpoint reachable by ChatGPT
- grant full normal Zodex agent access for the requested TTL
- when TTL expires, revoke MCP reachability, terminate active agent sessions, and stop the Local machine
- preserve the persistent Linux disk and workspace

The default TTL, if any, should be explicit in UX/docs and chosen conservatively. Support an operator-selected TTL such as `2d`.

#### `zodex local stop`

Immediately revoke ChatGPT access, terminate active agent sessions, and stop the Local machine while preserving its persistent disk.

#### `zodex local status`

Report at minimum:

- configured/not configured
- running/stopped
- MCP access active/inactive
- access expiry if active
- basic machine identity
- useful resource information if available

#### `zodex local reset`

Nuclear reset:

- revoke access
- terminate sessions
- stop/destroy the Local Linux machine
- destroy its agent filesystem/persistent disk
- recreate a pristine Local Zodex machine

This is intentionally stronger than `stop`.

### 7. Keep GitHub permissioning a separate capability

Local machine access and GitHub write authority are separate permissions.

Do not couple them.

The existing GitHub UX should remain familiar:

```bash
zodex github mode yolo \
  --local \
  --repo amxv/agentbox \
  --ttl 2d
```

and disable/status through the existing model:

```bash
zodex github mode default --local
zodex github mode status --local
```

Sprite remains supported exactly as before:

```bash
zodex github mode yolo \
  --sprite dev-sprite \
  --repo amxv/agentbox \
  --ttl 2d
```

Keep GitHub reader/publisher/direct-push behavior as close to the current implementation as possible. In particular, preserve the useful property that the agent does not use the user's normal GitHub CLI credentials, SSH agent, or host Git credentials.

Because `zodex-agent` remains unprivileged, the existing separation between agent and publisher can remain largely intact instead of moving publisher authority out to macOS solely for root-safety reasons.

### 8. Target resolution must fail closed when ambiguous

Do not introduce a global mutable "current target" whose meaning can silently change underneath commands or old sessions.

For GitHub operator commands:

- if exactly one eligible target exists, preserving current convenient inference is acceptable
- if both Sprite and Local are plausible, require explicit target selection
- never silently grant both

Preferred explicit selectors:

```text
--local
--sprite <name>
```

This preserves backward compatibility for existing Sprite-only users while remaining safe once Local is configured.

### 9. Use two separate MCP endpoint/app identities

Prefer two distinct ChatGPT MCP configurations:

```text
Zodex Sprite
Zodex Local
```

Both intentionally expose the same tool surface:

```text
exec_command
write_stdin
apply_patch
```

Do **not** add `target=local|sprite` to the MCP tool schemas.

Do **not** use one MCP URL that dynamically points to whichever machine is currently active.

Reasons:

- stable machine identity per MCP app
- prevents correct-command/wrong-machine failures
- keeps existing tiny tool schemas unchanged
- old conversations cannot silently jump from Sprite to Local or vice versa
- Sprite and Local can be used simultaneously by different conversations/tasks
- the model only needs to know which MCP app it is using, not infrastructure details on every tool call

The endpoint identity answers **which computer**. The existing three tools answer **how to operate it**.

### 10. Preserve the current MCP experience

The Local endpoint should reuse the same `zodexd` implementation and the same three tools wherever possible.

Do not add Local-only model-facing tools unless a concrete requirement makes them unavoidable.

The agent should not have to learn a second workflow. Once told to use Zodex Local, it should behave exactly as it does on Sprite:

```text
inspect /workspace
find/clone repo
pull latest changes
run commands/tests
apply patches
commit
push only if GitHub authority permits
```

Keep the existing MCP tool hints/annotations unchanged for this work unless another independent requirement calls for changing them.

### 11. Internet egress yes; macOS and private host network no

The Local Zodex Linux machine should have normal outbound Internet access so the agent can:

- reach GitHub
- use crates.io/npm/package registries
- download binaries and source archives
- access normal web APIs
- fetch dependencies

But Local Zodex must not thereby gain ambient access to the host or private network.

Default desired boundary:

```text
Internet                    allowed
GitHub/package registries   allowed
macOS filesystem            denied
macOS localhost/services    denied
Mac SSH agent               denied
Mac credentials/Keychain    denied
private LAN                 denied by default
```

For V1, the explicit trust boundary is:

- `zodex-agent` and every model-launched process are untrusted;
- `zodex-publisher` and `zodex-tunnel` retain separate identities and secrets;
- the Apple Container Linux kernel and root-owned Local control plane are trusted;
- macOS and Apple Container remain responsible for the VM/filesystem boundary.

The root-owned Local control plane must place `zodexd`, `zodex-prd`, `zodex-tunnel`, and every process they launch inside one dedicated Linux network namespace. That namespace has only a private veth connection to the trusted root namespace. Root-owned nftables rules match traffic arriving from the veth, drop access to the root namespace itself in an input chain, allow forwarding only to publicly routable IPv4 destinations, NAT that allowed traffic through the machine's provider interface, and deny macOS/vmnet addresses, private, loopback, link-local, carrier-grade NAT, multicast, documentation/benchmark, reserved, and other non-public ranges. V1 disables IPv6 in the agent namespace rather than allowing a second path whose macOS/global-address policy is harder to audit.

`zodex-agent` must have no sudo, `CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, or authority to enter, replace, reconfigure, or tear down the namespace, veth pair, forwarding policy, or nftables table. Operator commands through `zodex local exec` deliberately execute in the trusted root namespace and retain privileged administration/network access.

This is intentionally weaker than treating a compromised guest kernel or guest root as hostile. V1 promises isolation from the unprivileged coding agent, not containment after a Linux kernel exploit or compromise of the trusted root control plane.

### 12. No macOS ambient authority

The Local Zodex machine must not automatically receive:

- the user's macOS home directory
- arbitrary host folder mounts
- Keychain access
- SSH keys
- SSH agent forwarding
- Docker/container engine sockets
- host Git credentials
- shell dotfiles containing secrets
- host environment variables containing secrets
- host localhost access
- broad LAN access

The clean user-facing security statement should remain easy to understand, with the trusted-root qualification documented in the detailed security model:

> Zodex Local owns an isolated persistent Linux computer running on the Mac. ChatGPT can freely work inside that Linux computer and use the Internet, but it cannot access macOS.

---

## Sprite compatibility requirement

Adding Local must not degrade the existing Sprite path.

Protect the current Sprite experience:

- existing Sprite command names should continue to work
- Sprite-specific configuration such as org/service/proxy behavior should remain Sprite-specific
- do not force existing Sprite users through a new generic `target` command tree
- do not add Local parameters to commands that are inherently Sprite-only
- do not route Sprite through Local abstractions that make Sprite lifecycle harder to understand

A small internal target/provider abstraction is appropriate where behavior is genuinely shared, but the user-facing CLI should keep clear peer namespaces:

```text
zodex sprite ...
zodex local ...
```

Avoid over-generalization.

---

## Recommended internal decomposition

Use a narrow shared target abstraction only where it reduces duplication safely.

Conceptually:

```text
ExecutionTarget
  identity
  privileged_exec(...)
  start(...)
  stop(...)
  status(...)
```

Implementations:

```text
SpriteTarget
LocalTarget
```

Do not force every Sprite or Local operation through this interface. Provider-specific lifecycle details should stay provider-specific.

The major reusable areas should be:

- guest provisioning content
- Zodex runtime binaries/config
- `zodex-agent` identity and workspace ownership
- `zodex-prd` / publisher behavior
- Git credential/helper configuration
- MCP server/runtime
- session engine
- tool protocol
- GitHub policy/grant structures where feasible

Provider-specific areas should include:

- machine creation/destruction
- start/stop
- privileged operator exec transport
- MCP ingress
- network isolation
- resource allocation
- provider health/status queries

---

## Existing implementation observations that should guide the work

The current repository already has useful separation:

- `src/server.rs` exposes the exact three MCP tools through `ZodexService`
- `src/service.rs` is transport-independent enough to reuse directly
- `src/session/mod.rs` contains the actual command/session runtime
- `zodexd` runs agent commands as the configured `zodex-agent` identity in current deployments
- the installer creates separate `zodex-agent` and `zodex-publisher` users
- `/workspace` and `/home/zodex-agent` are already first-class runtime concepts
- plain `git push` is intercepted through `git-remote-zodex`
- direct push is converted to a Git bundle and brokered through the publisher path
- GitHub YOLO state already supports repo scopes and TTLs
- the current operator CLI's GitHub-mode implementation is somewhat Sprite-shaped internally (`ResolvedSprite`, `run_sprite_exec`), so Local support will require a careful target-level refactor without breaking Sprite

Also note: current `main` does not expose a public `zodex sprite exec` subcommand even though Zodex internally uses `sprite exec` and documentation shows raw `sprite exec -- sudo ...`. Local should nevertheless make the equivalent operator path first-class as `zodex local exec` because that UX is important for environment setup.

---

## Important invariants

1. **Same agent surface** — Local must not require a different MCP tool protocol.
2. **Stable machine identity** — `Zodex Local` always refers to the Local machine; `Zodex Sprite` always refers to the Sprite target.
3. **No silent target switching** — old sessions must never land on another machine because operator state changed.
4. **Agent remains unprivileged** — MCP execution remains `zodex-agent`, not root.
5. **Operator privilege stays out-of-band** — privileged Local maintenance is available only through the local operator CLI/control plane.
6. **Persistent by default** — stop/start must not discard repos, caches, packages, or build artifacts.
7. **Reset really resets** — reset destroys agent-controlled persistent state rather than attempting an in-place cleanup.
8. **No macOS mounts/secrets** — isolation must not depend on the model behaving nicely.
9. **Internet egress without host authority** — model-facing processes run inside the root-owned Local network namespace, so downloads/build dependencies work while macOS/private-host surfaces remain inaccessible to the unprivileged agent.
10. **GitHub authority is independent** — Local access does not imply GitHub push access.
11. **Sprite remains first-class** — Local is an additional target, not a rewrite that harms Sprite.
12. **Ambiguity fails closed** — where a command could affect multiple targets, require an explicit selector.

---

## Expected user workflow

One-time setup:

```bash
zodex local setup
zodex local exec -- sudo apt-get update
zodex local exec -- sudo apt-get install -y <common-dev-packages>
```

Normal recurring work:

```bash
zodex local start --ttl 2d
zodex github mode yolo --local --repo amxv/agentbox --ttl 2d
```

Then the user tells ChatGPT:

```text
Use Zodex Local. Pull agentbox and work on <task>.
```

ChatGPT sees the same tool style it already knows and works under `/workspace/repos`.

At the end or whenever desired:

```bash
zodex local stop
```

This preserves the entire Linux filesystem for next time.

For a complete wipe:

```bash
zodex local reset
```

Sprite remains independently usable through its own MCP app and existing operator commands.

---

## Non-goals for the first version

- exposing arbitrary macOS directories to ChatGPT
- running MCP commands directly on macOS
- giving ChatGPT root in the Local Linux machine
- per-repo VMs/capsules
- one disposable VM per task
- interactive local shell as a required V1 feature
- replacing or deprecating Sprites.dev support
- adding target parameters to MCP tools
- combining Local machine permission with GitHub push permission
- automatically forwarding host SSH/Git credentials
- allowing Local access to the private LAN by default
- building a menu bar app
- cross-platform support before the Apple Silicon implementation is solid

---

## Acceptance-level outcome

The design is successful when the user can keep both **Zodex Sprite** and **Zodex Local** configured in ChatGPT, choose either for a task, and the agent experience is effectively identical after selection.

For Local specifically, the user should be able to:

1. provision one persistent isolated Linux Zodex machine on an Apple Silicon Mac;
2. administer it as operator through `zodex local exec` without granting MCP root access;
3. start ChatGPT access with `zodex local start --ttl ...` and have access automatically revoked/stopped at expiry;
4. stop/restart the machine without losing repositories, packages, caches, or build outputs;
5. reset it completely with one explicit command;
6. run large Rust builds using the Mac's CPU/RAM while keeping build data on Linux-native persistent storage;
7. allow normal IPv4 Internet dependency access while the root-owned Local network namespace prevents the unprivileged agent from accessing macOS and the private LAN;
8. grant GitHub push independently through the existing YOLO model with an explicit `--local` selector when necessary;
9. preserve existing Sprite behavior and keep Sprite usable simultaneously through a separate MCP endpoint;
10. expose the exact same ChatGPT-facing `exec_command`, `write_stdin`, and `apply_patch` experience on both targets.

The guiding principle is simple:

> **Zodex Local is a local Sprite equivalent, not a new agent-computer product. Reuse the existing Zodex guest/runtime architecture wherever possible and change only the infrastructure layer that Sprites.dev currently provides.**
