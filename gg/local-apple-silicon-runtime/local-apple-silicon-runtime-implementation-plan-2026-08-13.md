# Local Apple Silicon Runtime — Implementation Plan

## Planning Basis

- **Repository:** `amxv/zodex`
- **Checkout inspected:** `/workspace/repos/zodex`
- **Approved branch:** `main`
- **Planning date:** 2026-08-13
- **Planning coordinate:** `6f35f14866842b6608887937825680231f90bf46` — a coordinate only; implementation agents must treat current remote code as authoritative.
- **Product specification:** [`local-apple-silicon-runtime-spec-2026-08-13.md`](./local-apple-silicon-runtime-spec-2026-08-13.md)
- **Current-state research:** [`local-apple-silicon-runtime-sweep-2026-08-13.md`](./local-apple-silicon-runtime-sweep-2026-08-13.md)
- **Planning protocol:** user-supplied Existing Repository Feature Planning Workflow, read in full.
- **Historical planning artifacts:** deliberately not consumed. Current code, tests, current product docs and selective Git history are the implementation evidence basis.
- **AgentBox provenance:** the accepted spec is from thread `thr_65fa716d-e0ba-4c10-9ab8-8972de3d7673`. The installed Sprite AgentBox CLI had a stale credential and returned `Unauthorized`; the exact thread body was retrieved through the authenticated AgentBox connector and stored in this workstream folder. No credential value is recorded here.

### Authoritative external sources used for unstable platform facts

Apple Container / container machine:

- `https://github.com/apple/container/blob/main/docs/container-machine.md`
- `https://github.com/apple/container/blob/main/docs/command-reference.md`
- `https://github.com/apple/container/blob/main/docs/how-to.md`
- `https://github.com/apple/container/blob/main/docs/technical-overview.md`
- `https://github.com/apple/container/blob/main/docs/network.md`

OpenAI Secure MCP Tunnel:

- `https://help.openai.com/en/articles/12584461-developer-mode-and-full-mcp-connectors-in-chatgpt-beta`
- `https://github.com/openai/tunnel-client`
- `https://github.com/openai/tunnel-client/blob/master/docs/connectors.md`
- `https://github.com/openai/tunnel-client/blob/master/docs/permissions.md`
- `https://github.com/openai/tunnel-client/releases`

These sources were checked on 2026-08-13. Apple Container and the tunnel client are evolving products; implementation must re-check the installed/current versions at the phase where behavior becomes load-bearing.

## State of Current System

Zodex already has the right agent-facing architecture for this feature. `zodexd` exposes the same command/stdin/patch computer service through MCP and HTTP without embedding a Sprite target in the protocol. Sprite-specific behavior primarily lives in the operator CLI: setup, privileged guest exec/uploads, provider service supervision, public proxying and target registry/resolution.

The existing runtime installer already has a provider-friendly split. `ZODEX_INSTALL_MODE=runtime` creates `zodex-agent` and `zodex-publisher`, installs the runtime binaries, creates `/workspace` and `/home/zodex-agent`, configures Git helpers and creates runtime TLS artifacts while allowing provider code to own lifecycle supervision. Sprite setup uses exactly that split.

The current GitHub architecture also largely fits Local: reader credentials, `git-remote-zodex`, publisher bundles and YOLO policy can remain guest-side so long as model execution remains unprivileged. The part that does not generalize yet is the **operator transport**: `github_mode.rs` directly accepts `ResolvedSprite` and calls `run_sprite_exec`.

The main current mismatches are documented in the Sweep:

- `workdir` is not a security boundary; Local must rely on machine isolation, not path filtering ([Landmine 1](./local-apple-silicon-runtime-sweep-2026-08-13.md#1-workdir-is-not-a-filesystem-security-boundary)).
- Apple container-machine can map the macOS home by default; that contradicts the spec ([Landmine 2](./local-apple-silicon-runtime-sweep-2026-08-13.md#2-apple-container-machines-default-home-mapping-contradicts-the-requested-boundary)).
- public Internet with host/private-LAN denial is not yet proven by Apple's documented machine surface ([Landmine 3](./local-apple-silicon-runtime-sweep-2026-08-13.md#3-public-internet-without-private-lan-is-not-currently-proven-by-apple-machine-docs)).
- the direct Linux systemd unit omits `User=`/`Group=`, while Sprite Services and process mode explicitly run the agent unprivileged ([Landmine 4](./local-apple-silicon-runtime-sweep-2026-08-13.md#4-the-direct-systemd-unit-does-not-specify-the-agent-user)).
- runtime-only install deliberately leaves provider service lifecycle outside the installer ([Landmine 5](./local-apple-silicon-runtime-sweep-2026-08-13.md#5-runtime-only-install-deliberately-leaves-provider-lifecycle-outside-the-installer)).
- current GitHub YOLO installation is Sprite-coupled at the transport boundary ([Landmine 7](./local-apple-silicon-runtime-sweep-2026-08-13.md#7-github-mode-policy-code-is-target-independent-in-concept-but-sprite-coupled-in-transport)).
- there is no durable host-side machine-access lease supervisor ([Landmine 9](./local-apple-silicon-runtime-sweep-2026-08-13.md#9-there-is-no-existing-durable-machine-access-lease-supervisor)).

The Sweep carries the detailed call paths, persistence, tests and provider evidence.

## State of Ideal System

After all phases, Zodex has two peer execution targets while retaining one agent-computer protocol:

```text
                         ChatGPT
                            │
                 same 3 MCP tools/schemas
                            │
             ┌──────────────┴──────────────┐
             │                             │
       Zodex Sprite                  Zodex Local
             │                             │
     Sprite public proxy          Secure MCP Tunnel
             │                             │
      Sprite Services             zodex-tunnel service
             │                             │
             └──────────────┬──────────────┘
                            │
                   same guest runtime
                            │
             ┌──────────────┴──────────────┐
             │                             │
       zodexd as agent             zodex-prd as publisher
             │                             │
        /workspace/repos                 GitHub
```

### Cross-phase vocabulary

These names are the canonical concepts implementation agents should preserve across phases. Phase-local helper names remain implementation details.

**Local target** — the single persistent Apple-Silicon-hosted Linux Zodex machine supported by V1. Its stable provider machine identity is `zodex-local` unless an implementation-time provider restriction requires a different fixed name.

**`LocalTargetRecord`** — durable operator-side nonsecret setup metadata for the Local target. It records enough source references and expected machine configuration to inspect, repair and reset the Local target without storing PEM contents, tunnel runtime-key contents or other secret values.

**`LocalAccessLease`** — durable operator-side state for the current ChatGPT-access window: a generation/identity, creation time, absolute expiration and active/inactive state. The lease is not GitHub authority and does not contain the MCP/tunnel key.

**Local machine provider** — the Apple Container adapter responsible for machine capability detection, create/inspect/start/stop/remove, root operator exec and safe data transfer. It does not own MCP schemas or GitHub policy.

**Local guest services** — provider-private systemd services installed into the Local machine:

- `zodexd` running as `zodex-agent`;
- `zodex-prd` running as `zodex-publisher`;
- the Secure MCP Tunnel client running as a separate restricted `zodex-tunnel` identity.

The tunnel service is disabled/inactive by default. Merely booting the Linux machine, including through `zodex local exec`, does not grant ChatGPT access.

**Operator guest target** — the narrow internal abstraction needed by operator-only code that must execute privileged commands and transfer bounded state into either a Sprite or the Local machine. It is not a user-facing generic `target` command tree and must not absorb provider-specific lifecycle operations.

### Final Local lifecycle

```text
zodex local setup
  ├─ validate Apple Silicon/macOS/container-machine support
  ├─ create/reconcile one persistent machine with no host-home mount
  ├─ provision runtime-only Zodex guest
  ├─ provision reader/publisher credentials and Git setup
  ├─ install provider-private zodexd/zodex-prd/tunnel services
  ├─ verify agent/publisher/tunnel secret boundaries
  └─ leave ChatGPT access OFF

zodex local exec -- <command...>
  ├─ boot machine if necessary
  ├─ execute operator command with provider root authority
  └─ never start the MCP tunnel implicitly

zodex local start --ttl 2d
  ├─ require a finite TTL
  ├─ reconcile stale/expired lease state
  ├─ verify machine + host/network isolation invariants
  ├─ start/repair zodexd + zodex-prd
  ├─ start tunnel service
  ├─ persist a new generation LocalAccessLease
  └─ arm host-side durable expiry supervision

expiry or zodex local stop
  ├─ revoke tunnel ingress first
  ├─ stop the Linux machine, killing all agent sessions
  ├─ mark/clear access lease
  └─ preserve persistent machine storage

zodex local reset
  ├─ prove all required reprovisioning inputs are still available
  ├─ revoke/stop current access
  ├─ destroy the Linux machine and its persistent disk
  └─ recreate/reprovision a clean Local target
```

### Final security boundary

The final user-understandable claim is:

> Zodex Local owns an isolated persistent Linux computer running on the Mac. ChatGPT can freely work inside that Linux computer and use the public Internet, but it cannot access macOS or the private LAN.

The machine boundary, not `workdir`, enforces that claim. No macOS home or project folder is mounted into the guest. Host SSH agent, Keychain, Git credentials, shell configuration and container sockets are not forwarded.

`zodex-agent` remains unprivileged. It can use its home and `/workspace`, run large builds, install user-space tools and access the Internet. Only operator-side `zodex local exec` has privileged guest administration.

`zodex-publisher` remains separate and retains the current writer-key isolation. `zodex-tunnel` is a third restricted service identity so model commands cannot read the OpenAI tunnel runtime credential or make access outlive the host lease.

### Final MCP identity

Sprite and Local are separate ChatGPT app/MCP endpoint identities. Both expose the same existing three tools without a provider parameter. An old conversation connected to one endpoint can become unavailable, but it can never silently begin operating the other computer.

### Final GitHub identity

GitHub authority remains independent of Local access:

```bash
zodex github mode yolo --local --repo amxv/agentbox --ttl 2d
zodex github mode status --local
zodex github mode default --local
```

Sprite spellings continue unchanged. When target inference would be ambiguous between configured Local and Sprite targets, operator GitHub commands fail closed and require `--local` or `--sprite <name>`.

## Decisions and Assumptions

### Decisions

#### 1. Use one persistent Apple container machine, not per-repo containers or host mounts

**One-way product boundary for V1.**

- **A. One persistent `container machine` with its own Linux filesystem.** — **Recommended**
- **B. One container/VM per repo.** More lifecycle state, weaker cache reuse and a different mental model from Sprite.
- **C. Mount macOS repositories into Linux.** Violates the approved host-isolation model and weakens build-storage performance.

**Why A:** current Zodex already assumes a persistent agent computer with `/workspace`; Apple's machine model preserves filesystem state across stop/start and gives the Local target the user's desired CPU/RAM headroom without giving the model host filesystem authority.

**Selected:** A.

#### 2. Use a provider-private Ubuntu/systemd machine image and the existing runtime-only Zodex installer

**Reversible implementation choice, load-bearing for service ownership.**

- **A. Minimal reviewed Ubuntu 24.04-or-newer systemd image plus `ZODEX_INSTALL_MODE=runtime`.** — **Recommended**
- **B. Run the existing direct `zodex install/start` systemd path inside the machine.** The current direct unit can run `zodexd` as root and conflates provider lifecycle with guest install.
- **C. Create a second Local-specific runtime implementation.** Duplicates the existing users/Git/runtime logic.

**Why A:** Sprite already proves the runtime installer is the reusable boundary. Local should install its own provider-private service units with explicit identities rather than changing the agent runtime.

**Selected:** A.

The machine image/build recipe must be embedded or otherwise shipped with the macOS operator binary so release users do not require a source checkout merely to run `local setup`.

#### 3. Keep model execution unprivileged; make `local exec` the privileged operator path

**One-way security boundary for V1.**

- **A. `zodexd` as `zodex-agent`; operator `local exec` uses provider root authority.** — **Recommended**
- **B. Give `zodex-agent` sudo/root.** Simplifies occasional package installation but collapses publisher/tunnel secret boundaries.

**Why A:** root provides no build-performance advantage. The user explicitly prefers the same one-time environment-setup pattern already used with Sprite.

**Selected:** A.

`zodex local exec -- sudo apt-get ...` remains accepted UX even if the provider transport itself is already executing as root; the command tokens are passed transparently and `sudo` is present in the base machine image.

#### 4. Keep Local operator state separate from guest `/etc/zodex/config.toml`

**Reversible storage choice, cross-phase contract.**

- **A. Dedicated nonsecret operator records under the existing `~/.config/zodex/` namespace.** — **Recommended**
- **B. Reuse the guest `Config` file path or Sprite registry.** Conflates host target state with Linux runtime state or provider-specific Sprite identity.

**Selected:** A.

Use separate durable records for Local setup metadata and Local access lease state. Write them atomically with user-only permissions. Store **paths/references** to PEM/tunnel secret files, never their contents.

The setup record must distinguish a ready target from interrupted/provisioning state so a partial machine is never silently treated as safe/complete.

#### 5. V1 supports exactly one Local target with stable identity

**Reversible future product choice.**

- **A. One Local machine (`zodex-local`).** — **Recommended**
- **B. Named/multiple Local machines in V1.** Adds a registry/selection problem that is not needed by the accepted spec.

**Why A:** the user wants one persistent local equivalent of their current Sprite machine. Sprite already covers multiple remote machines. The target abstraction should not pre-emptively expose naming UX for a requirement that does not exist.

**Selected:** A.

#### 6. `local setup` reuses Sprite-style GitHub provisioning inputs and stores only source references

**Reversible CLI detail.**

The Local setup path should accept the same GitHub App material necessary to reproduce the current guest access model: seed `--repo`, reader App ID/PEM, publisher App ID/PEM and default base. It additionally accepts the Secure MCP Tunnel identity/runtime-secret source plus optional machine resource overrides.

For the tunnel credential, prefer a **secret file input/reference**, not a literal key argument. The persisted Local setup record keeps only the source reference/path. Setup copies the secret into a guest file owned only by `zodex-tunnel`; logs/status must never print it.

#### 7. Run Secure MCP Tunnel inside the Local Linux machine as `zodex-tunnel`

**Reversible architecture choice, strongly preferred.**

- **A. Guest-side outbound tunnel service.** — **Recommended**
- **B. macOS-side tunnel proxying into a guest port.** Requires an additional host-to-machine transport/port boundary and depends on provider reachability not needed by the product.
- **C. Make `zodexd` publicly reachable.** Violates the Local/private design and current ChatGPT localhost constraint.

**Why A:** OpenAI ships Linux ARM64 tunnel-client releases and the tunnel is outbound. Keeping it in the guest means the Mac does not need to expose a guest daemon port. A separate OS identity preserves lease/secret authority from model commands.

**Selected:** A.

Local `zodexd` should bind only to guest loopback for this transport. The tunnel connects to the existing Local MCP endpoint over guest-local HTTP/TLS; no MCP schema change is permitted.

The tunnel service must not auto-start merely because the machine boots.

#### 8. `local start` always requires a finite TTL; Local has no YOLO/default/no-TTL mode

**User-approved public behavior.**

- **A. `zodex local start --ttl <duration>` required; no Local `--no-ttl`.** — **Recommended**
- **B. Add a default/no-TTL Local access mode.** Reintroduces ambiguous authority language and weakens automatic disconnect.

**Why A:** the user explicitly preferred start/stop lifecycle vocabulary over `mode yolo/default`. Existing duration parsing already supports `s/m/h/d`, including `2d`.

**Selected:** A.

Refactor the existing TTL parser toward provider-neutral naming/usage rather than creating a second grammar.

#### 9. Local access expiry is host-owned durable state supervised outside the guest

**One-way security ownership boundary.**

- **A. Durable `LocalAccessLease` plus a macOS launchd-supervised hidden worker/reconciler.** — **Recommended**
- **B. Guest timer/systemd unit.** Model-controlled machine state should not be authoritative for whether the host permits ChatGPT access.
- **C. Detached `nohup`/sleep from the CLI.** Fragile across terminal exit, crashes, sleep and relaunch.

**Why A:** a TTL must have an active side effect: tunnel shutdown and machine stop. The macOS operator plane is the authority that can always stop the provider machine. Generation-checked durable state prevents an old expiry worker from stopping a newly renewed lease.

**Selected:** A.

The exact launchd plist/process mechanics are phase-local implementation details; the contract is that expiry survives the initiating CLI process, reconciles after sleep/restart, and cannot let a stale generation revoke a newer one.

A Mac reboot must **not** automatically re-expose Local merely because a pre-reboot TTL remains. The next explicit `local start` creates/reconciles access.

#### 10. Stop revokes tunnel first, then stops the machine

**Safety ordering.**

Manual stop and TTL expiry share one authoritative stop/reconcile path:

1. make new MCP traffic unavailable by stopping/disabling tunnel ingress;
2. stop the Linux machine, which terminates `zodexd` and all session processes;
3. durably mark the access lease inactive/cleared;
4. preserve the machine disk.

Repeated stop is idempotent. An old MCP `session_handle` is expected to become invalid after a stop/restart.

#### 11. Private-LAN denial is a hard provider acceptance gate, not a best-effort guest firewall

**One-way security requirement from the spec.**

Current Apple docs do not prove the exact network policy. Implementation must establish a host/provider-enforced mechanism and demonstrate it live before `local setup/start` can be considered safe.

If the current Apple Container version cannot enforce the boundary in a maintainable host-controlled way, the phase becomes **blocked** and the plan must receive an Amendment selecting a different host-enforced networking mechanism/provider arrangement. Do not silently weaken the goal and do not treat a guest-only firewall as authoritative.

`local start` must fail closed when its known network/isolation invariants are absent or have drifted.

#### 12. GitHub mode gets a narrow operator-target abstraction, not duplicated Local policy

**Long-term architecture decision.**

- **A. Factor privileged exec/bounded upload needed by GitHub mode across Sprite and Local.** — **Recommended**
- **B. Copy `enable/disable/status` into Local-specific functions.** Creates two policy implementations.
- **C. Convert the entire operator CLI to a generic `target` abstraction.** Over-generalizes and harms Sprite-specific clarity.

**Selected:** A.

Provider-specific lifecycle remains under `zodex sprite ...` and `zodex local ...`. The shared abstraction exists only where operator policy genuinely needs to manipulate the same guest state.

#### 13. GitHub target selection fails closed when Local and Sprite are both plausible

**User-approved public behavior.**

Add `--local` to operator GitHub mode commands while keeping existing `--sprite` behavior. The selectors are mutually exclusive.

If only one eligible target is configured, preserving convenient inference is allowed. If Local and Sprite are both plausible and neither selector is passed, return an actionable ambiguity error rather than modifying both or choosing one.

No global mutable “current target” is introduced.

#### 14. `local reset` is destructive recreation with preflight, not in-place cleanup

**User-approved destructive behavior.**

Before deleting the machine, reset must prove that every nonsecret setup reference required for recreation is resolvable and all referenced credential/tunnel secret files are currently readable. If preflight fails, leave the existing machine intact.

Once deletion begins, reset follows the same setup/reprovision contract and results in a clean `/workspace`/agent home with the requested runtime and service policy. It does not attempt to preserve repos or build caches.

#### 15. Resource tuning is optional setup state, not an MCP concern

Expose Local CPU/memory choices on the operator setup/reconcile surface where the Apple provider supports them. If omitted, documented provider defaults may be used. Persist requested overrides in the Local setup record so reset reproduces them.

The MCP agent cannot change host machine allocation through a model-facing tool.

#### 16. Sprite and Local remain separate MCP identities with exactly the same tool surface

**User-approved product boundary.**

No `target` field is added to `ExecCommandInput`, `WriteStdinInput` or `ApplyPatchInput`. Existing descriptions/annotations remain unchanged. Sprite and Local can be connected simultaneously in different conversations.

### Factual unknowns and working assumptions

#### A1. Apple Container supports the machine lifecycle shapes documented today on the implementation Mac

- **Unknown:** exact installed/current Apple Container version and whether CLI JSON/inspect shapes match the 2026-08-13 docs.
- **Working assumption:** implementation targets Apple Silicon + macOS 26 with a container release that includes `container machine` create/run/inspect/start/stop/set/rm and explicit no-home-mount support.
- **Why reasonable:** those are current first-party documented features and the accepted spec is Apple-Silicon-first.
- **If false:** Phases 1–2 must amend provider/version requirements before exposing Local setup.
- **Settled by:** live provider preflight on the target Mac plus deterministic fixture/parser tests.

#### A2. A host-controlled network arrangement can allow public Internet while denying host/private-LAN access

- **Unknown:** exact supported Apple Container/vmnet mechanism for this asymmetric egress policy.
- **Working assumption:** the Apple/macOS provider layer can enforce it without trusting guest root configuration.
- **Why reasonable:** Apple's stack is built over host networking/vmnet, but the high-level machine CLI does not document this exact policy.
- **If false:** Phase 2 blocks and adds a plan Amendment; this may change Local provider/network implementation substantially.
- **Settled by:** live security probe from the actual Local machine against public Internet, macOS host/gateway and reachable private address space.

#### A3. Secure MCP Tunnel remains available with a Linux ARM64 client and Streamable HTTP connector

- **Unknown:** exact stable tunnel-client version when Phase 3 lands.
- **Working assumption:** current capability remains supported.
- **Why reasonable:** it is current OpenAI-documented functionality and current releases include Linux ARM64.
- **If false:** Phase 3 must amend only the ingress mechanism; the Local guest/runtime and MCP schemas should remain unchanged.
- **Settled by:** re-reading official docs/releases and a live tunnel session in Phase 3.

#### A4. The operator can provide a tunnel ID and restricted runtime key without giving Zodex an OpenAI admin key

- **Unknown:** the exact pre-created tunnel/account state on the implementation Mac.
- **Working assumption:** the user can create/provide the tunnel identity and a restricted runtime key file.
- **If false:** setup UX may require additional operator guidance, but Zodex must not solve it by storing an admin key.
- **Settled by:** Phase 3 live setup.

#### A5. An explicitly authorized disposable GitHub ref/repository will be available for end-to-end Local push evidence

- **Unknown:** which target is safe for destructive live push tests during implementation.
- **Working assumption:** deterministic policy tests can land first; the final acceptance run will use an operator-authorized disposable branch/ref when available.
- **If false:** GitHub live write evidence remains a named acceptance gap; no unauthorized push is performed.
- **Settled by:** operator-provided test scope at Phases 4/7.

#### A6. Large Rust builds benefit from the Local machine without requiring a host bind mount

- **Unknown:** exact speedup versus Sprite or native macOS.
- **Working assumption:** configured local CPU/RAM plus persistent guest storage provides the required headroom.
- **If false:** performance tuning may change resource/storage choices but must not weaken host isolation.
- **Settled by:** representative live Rust build and stop/start persistence evidence in Phase 7. No numeric speedup is assumed.

## Acceptance Criteria

1. On a supported Apple Silicon Mac, `zodex local setup` can provision exactly one persistent Local Zodex Linux machine without requiring a source checkout beside the installed operator binary.
2. Local setup fails with an actionable unsupported-platform/provider error on non-Apple-Silicon, unsupported macOS, missing/incompatible Apple Container, or missing required machine capability rather than partially provisioning an unsafe target.
3. The Local Linux machine does not mount the user's macOS home or any arbitrary macOS project directory, and setup/start verify the no-host-home invariant before granting ChatGPT access.
4. `/workspace`, `/home/zodex-agent`, repositories, build artifacts and language/package caches live on the Local machine's persistent Linux storage rather than a macOS bind mount.
5. Stopping and restarting the Local machine preserves an agent-created sentinel, Git repository data and build/cache state in `/workspace`/agent home.
6. `zodexd` executes MCP commands as `zodex-agent`, and the agent has no general sudo/root capability.
7. `zodex-prd` executes as `zodex-publisher`, and `zodex-agent` cannot read the publisher private key or otherwise acquire publisher installation credentials.
8. The Secure MCP Tunnel runs under a separate restricted guest identity, and `zodex-agent` cannot read the tunnel runtime credential or modify the authoritative host access lease.
9. `zodex local exec -- <command...>` is an operator-only non-interactive path that can run privileged guest maintenance commands, including the documented `sudo apt-get ...` setup pattern, without exposing that privilege through MCP.
10. `zodex local exec` can boot/use the persistent Local machine when necessary but never starts or re-enables the Secure MCP Tunnel by itself.
11. A successful Local setup verifies normal public-Internet egress from the guest for dependencies such as GitHub/package registries.
12. Before Local access is accepted, live evidence proves guest access to macOS host services and the private LAN is denied by a host/provider-controlled boundary; inability to prove/enforce that boundary blocks the feature rather than weakening it to guest-only firewalling.
13. Local `zodexd` is not broadly exposed on the Mac/LAN; the intended MCP ingress is the separate outbound Secure MCP Tunnel.
14. `zodex local start` requires an explicit finite `--ttl` and accepts the existing Zodex `s/m/h/d` duration grammar, including `2d`.
15. `zodex local start --ttl ...` starts/repairs the guest runtime, passes isolation preflight, starts the tunnel, records a durable absolute-expiry lease and reports the resulting Local MCP/tunnel identity without printing secret values.
16. Merely having the Local machine running is distinguishable from having ChatGPT access active; `local status` reports both states truthfully.
17. Manual `zodex local stop` makes the Local MCP endpoint unavailable, terminates active agent processes/sessions by stopping the machine, clears/reconciles the access lease and preserves persistent machine storage.
18. TTL expiry performs the same effective revocation/stop even after the initiating `zodex local start` process has exited.
19. If the Mac sleeps past expiry or the lease worker is restarted, the next reconciliation does not leave an expired tunnel/machine access window active.
20. A Mac reboot does not automatically restore ChatGPT Local access solely because a prior lease had remaining wall-clock time; a new explicit `local start` is required.
21. Re-running `local start` before expiry renews access through a new lease generation, and a stale expiry worker from the previous generation cannot stop the renewed session early.
22. Repeated `local stop`, stale/incomplete lease state and an already-stopped machine are handled idempotently with truthful status rather than destructive errors.
23. Session handles returned before a Local stop/expiry are stale after restart; no UI/CLI output represents them as resumable across machine stop.
24. Local uses the existing `exec_command`, `write_stdin` and `apply_patch` MCP schemas, names, descriptions and annotations without adding a target/provider field.
25. A ChatGPT connection to Zodex Local can run the same normal workflow as Sprite: inspect `/workspace`, clone/pull, execute long-running commands with polling, patch files and commit locally.
26. Zodex Sprite and Zodex Local can be configured as separate MCP app identities simultaneously, and changing/stopping one target never silently routes an existing connection to the other.
27. Local machine access does not grant GitHub direct-push authority by itself.
28. `zodex github mode yolo --local --repo <owner/repo> --ttl <duration>` applies the existing YOLO policy/TTL semantics to the Local guest without copying a second policy implementation.
29. `zodex github mode status --local` truthfully reports Local YOLO scope/expiry and direct-push readiness, and `zodex github mode default --local` removes only Local YOLO state while leaving unrelated explicit grants unchanged as today.
30. Existing `zodex github mode ... --sprite <name>` behavior, all-installed/repo-merge TTL semantics and Sprite Git helper repair continue to work unchanged.
31. `--local` and `--sprite` are mutually exclusive. When both Local and Sprite are plausible and neither selector is supplied, target-dependent operator GitHub commands fail closed with actionable guidance instead of selecting or modifying both.
32. When exactly one eligible execution target exists, existing convenient target inference may continue without creating a global mutable current-target setting.
33. Local reader-backed clone/fetch, PR publishing and direct push use the existing Zodex GitHub App/helper/publisher architecture and do not forward the user's host `gh` login, SSH agent, SSH keys or normal Git credential store.
34. With Local repo-scoped YOLO active and an explicitly authorized live test ref, normal `git push` from the agent succeeds only for the allowed repo/scope; the same push fails after Local YOLO is disabled or expired.
35. `zodex local status` reports not-configured/provisioning/ready state, provider machine running/stopped state, access active/inactive/expired state, expiry when active, machine identity and available resource configuration without leaking API, GitHub or tunnel secrets.
36. `zodex local setup` is safely repeatable for an existing ready machine: it repairs/reconciles Zodex-owned runtime configuration without deleting `/workspace`, user-installed development packages or build caches.
37. An interrupted Local setup is represented as incomplete/needs-repair rather than ready, and a later setup/status path can reconcile or diagnose it without direct manual state-file editing.
38. `zodex local reset` preflights all required setup references and secret files before deleting anything; missing reprovisioning inputs cause a fail-closed error that leaves the current machine intact.
39. A successful `local reset` revokes access, destroys the Local machine and persistent agent filesystem, recreates/reprovisions the target from saved setup references, and proves pre-reset workspace data is gone.
40. Optional operator CPU/memory overrides are applied through the Apple provider where supported, reported by status and retained as reset/reprovisioning intent; MCP cannot change them.
41. A representative large Rust project can build inside Local using the configured Mac-backed CPU/RAM, and its build artifacts/cache remain reusable after a Local stop/start cycle; no numeric speedup over Sprite/native macOS is required.
42. Local setup/start never stores raw GitHub PEM contents or OpenAI runtime keys in operator JSON state, command output, logs, Git-tracked files or process command lines when a file/secret reference mechanism is available.
43. The macOS operator release/install path continues to produce and install `aarch64-apple-darwin` Zodex successfully; Local does not require a second distribution mechanism.
44. Existing Sprite setup, upgrade, service sync, status, logs, proxy and health workflows remain first-class and their current tests continue to pass.
45. User-facing README/docs/help explain Local setup, operator exec, start/stop/TTL, reset, separate MCP identity, resource/persistence behavior, network boundary and `github mode --local` without presenting implementation-phase history.
46. Repository-wide validation, source-size guard, relevant docs build/check and packaging/install smoke pass at final acceptance, and no test fixture/secret/private host material is committed.

## Plan Phases

### Execution protocol

Implementation agents must treat the current remote `main` branch as truth. Before each phase, fetch first, fast-forward safely, preserve newer work and re-open the exact symbols named by that phase. Never reset, force-push or discard another agent's work.

A phase is complete only when its observable capability, phase-specific positive evidence, relevant regressions, required live evidence, applicable docs/generated-contract updates, progress-ledger update, coherent commit, normal push, post-push fetch/remote equality and clean-worktree check are complete.

If implementation reality invalidates a load-bearing assumption or approach, add an **Amendment** immediately. Amendments may change implementation technique, never weaken the accepted security/product outcomes. In particular, inability to enforce private-LAN denial is a blocker requiring an architecture amendment, not permission to ship a weaker boundary.

Do not expose planning phase numbers in user-facing docs or CLI output.

### Phase 1 — Local provider/state seam and truthful read-only status

#### Files to read before starting

**Orientation / current contracts**

- `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-spec-2026-08-13.md` — read the accepted Local/Sprite product boundary in full.
- `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-sweep-2026-08-13.md` — read **Operator CLI module shape**, **Persistence and identity**, **Public API / CLI / UI / schema / generated contracts**, and Landmines 2, 6, 13, 14, 18.
- `src/bin/zodex/prelude.rs` — `Commands`, `SpriteCommand`, target/state structs and host-path constants; understand the current public CLI/data home before adding Local vocabulary.
- `src/bin/zodex/dispatch.rs` — `run`; understand command routing and Linux-only gates.

**Patterns to copy**

- `src/bin/zodex/credentials.rs` — `operator_sprites_registry_path*`, registry load/save/upsert, `resolve_remote_sprite*`, `parse_push_grant_ttl`; copy atomic/fail-closed target semantics where appropriate and generalize duration parsing without inventing a second grammar.
- `src/bin/zodex/sprite_proxy.rs` — `build_sprite_scope_args`, `run_sprite_exec`; understand current provider command-runner style without moving Sprite lifecycle behind a generic target abstraction.

**Contracts/tests at risk**

- `tests/zodex_operator_cli.rs` — current help expectations.
- `src/bin/zodex/tests/part2.rs` — Sprite registry resolution and TTL parser tests.
- `tests/source_file_size.rs` — module-size constraint that should shape new Local responsibility boundaries.
- `scripts/install.sh` — `detect_operator_platform`, `run_operator_install`; confirm the operator binary already supports Apple Silicon/macOS.
- `.github/workflows/release.yml` — current cross-platform compile/package matrix.

**External provider facts to re-check**

- Apple `container-machine.md` and command reference listed in Planning Basis — verify current machine create/inspect/run/start/stop/set/rm and `home-mount=none` spellings before coding parser/runner shapes.

#### What to do

Establish a Local-provider module boundary and durable host-state model without touching the existing MCP protocol or Sprite public command names.

Add the `zodex local` namespace with a truthful `status` capability first. Introduce responsibility-separated code for:

- Local platform/provider capability detection;
- safe invocation/parsing of the Apple `container` CLI;
- the single stable Local machine identity;
- atomically loaded/saved `LocalTargetRecord` and `LocalAccessLease` records under the existing user-owned Zodex config namespace;
- target readiness states that distinguish `not configured`, interrupted/provisioning, ready and provider/machine drift;
- generic duration parsing shared with existing GitHub TTL consumers.

`local status` must be read-only. Before setup it should say not configured and report actionable missing/unsupported provider prerequisites. If a machine with the fixed Local identity already exists, status should report its observable running/resources/home-mount state without silently adopting it as a ready Zodex target.

Keep Apple-specific command invocation behind a narrow provider runner so deterministic tests can exercise command construction/parsing on Linux CI. Do not hide or rewrite Sprite commands behind this provider layer.

Do not add `setup`, `start`, `exec` or GitHub `--local` as partially functional public promises in this phase unless their help can truthfully state they are not yet available. Prefer landing each public subcommand only when its vertical behavior is implemented.

Do not put raw secrets into the Local host records. Define future source-reference fields/types only as needed for Phase 2 compatibility; do not invent an encrypted-secret store.

#### Validation strategy

**Positive evidence**

- New deterministic tests prove Apple command construction/parsing, platform/version/capability classification and no-home-mount state classification using representative provider output fixtures.
- New CLI tests prove `zodex local --help`/`zodex local status` exist and status distinguishes not-configured from unsupported/missing-provider state without mutating the machine.
- TTL tests prove the same `s/m/h/d` parser used by GitHub accepts `2d` for future Local leases.
- On the target Apple Silicon Mac, run `zodex local status` against the actual installed Apple Container version and record the observed version/capability shape. This is a read-only live provider probe.

**Regression evidence**

- Existing Sprite registry explicit/env/single/ambiguous tests remain green.
- Existing `zodex github mode ... --help` still exposes the current Sprite flags and TTL behavior.
- The operator binary still compiles/tests on Linux CI paths; Local unsupported-platform behavior is runtime-gated rather than removing shared CLI code from the build.

#### What must not break

- Existing Sprite registry path/data and `ZODEX_SPRITE` behavior.
- Existing GitHub TTL grammar/expiry semantics.
- Existing macOS operator installation path.
- Current `exec_command`/`write_stdin`/`apply_patch` schemas and annotations.
- Source-file-size guard and responsibility-based module organization.

### Phase 2 — Persistent Local machine setup and privileged operator exec

#### Files to read before starting

**Current guest provisioning to reuse**

- `scripts/install.sh` — `run_runtime_install`, `ensure_service_accounts`, `ensure_dirs_and_config`, `configure_agent_git_identity`, `configure_agent_git_reader_helper`; this is the canonical reusable guest foundation.
- `src/bin/zodex/sprite_proxy.rs` — `sprite_setup`, `build_sprite_setup_script`, `verify_agent_git_identity`, `verify_reader_git_access`, `verify_publisher_socket_permissions`, `verify_publisher_key_isolation`; copy behavior/verification, not Sprite transport.
- `src/bin/zodex/status.rs` — `expected_sprite_service_definitions`; preserve the explicit `zodex-agent`/`zodex-publisher` identities.
- `src/bin/zodex/process.rs` — `daemon_launch_command`, agent/publisher ownership helpers; confirms the non-root contract outside Sprite Services.
- `src/config.rs` — guest defaults and runtime paths.

**Current lifecycle path not to copy blindly**

- `src/bin/zodex/lifecycle.rs` — `install`, `start_stack`, `ensure_linux`; understand why Local provider lifecycle must remain outside direct guest lifecycle.
- `src/bin/zodex/status.rs` — `render_systemd_unit`; Landmine 4 explains why this direct unit is not the Local authority model.

**Provider/state foundation**

- Current Phase 1 Local provider/state modules and tests — read in full; they are now the provider truth.
- Sweep sections **Authentication and secret handling**, **External / provider / platform boundaries**, **Existing patterns worth copying**, and Landmines 1–5, 10–12.

**Packaging / runtime availability**

- `.github/workflows/release.yml` — Linux ARM64 runtime and macOS ARM64 operator artifacts.
- `tests/install_script.rs`, `tests/sprite_scripts.rs` — installer/Sprite regression contracts.

**External provider facts**

- Re-check Apple machine image/create/run documentation, especially `/sbin/init`, home-mount disabling, persistent machine storage, root exec and CPU/memory flags.

#### What to do

Implement `zodex local setup` and `zodex local exec` as the first complete Local machine slice.

`local setup` must:

- require the supported Apple Silicon/macOS/container-machine capability set;
- create or reconcile the one stable persistent Local machine using a reviewed systemd-capable Linux image;
- embed/ship the machine image/build recipe with the operator distribution rather than assuming a source checkout;
- explicitly disable host-home mounting and create **no** arbitrary host directory mounts;
- apply optional CPU/memory overrides while preserving provider defaults when omitted;
- reuse the runtime-only Zodex installer for users, binaries, `/workspace`, Git helpers and TLS;
- provision the same reader/publisher GitHub App model as Sprite using operator-supplied source PEM paths;
- install **provider-private** Local guest service units with `zodexd` explicitly running as `zodex-agent` and `zodex-prd` explicitly running as `zodex-publisher`;
- bind Local `zodexd` only to guest-local/loopback addresses needed by the future tunnel, not the host/LAN;
- create/provision the restricted `zodex-tunnel` identity and required directories so Phase 3 can install/start ingress without widening agent privileges;
- verify workspace ownership, Git identity, reader access, publisher socket modes and publisher-key isolation using Local provider exec rather than Sprite exec;
- atomically mark the Local setup record ready only after those checks and the network/isolation gate below pass;
- leave ChatGPT/tunnel access inactive.

`local exec -- <tokens...>` must execute operator-controlled commands through the Apple machine provider with privileged/root guest authority. Preserve argument boundaries; do not construct an avoidable shell string that can change caller intent. It may boot a stopped persistent machine as part of provider exec. It must not start the future tunnel or mark Local access active.

Setup should be idempotent/reconciling when rerun on a ready machine: refresh Zodex-owned runtime/service/Git configuration and requested resource settings without deleting `/workspace`, language caches, installed development packages or build outputs.

#### Validation strategy

**Positive deterministic evidence**

- Tests prove machine creation always requests no home mount and never includes a macOS workspace/home bind.
- Tests prove setup record writes are atomic/permission-controlled and interrupted state cannot be mistaken for ready.
- Tests prove Local service definitions run `zodexd` as `zodex-agent` and `zodex-prd` as `zodex-publisher`, with tunnel identity separate.
- Tests prove `local exec` preserves command-token boundaries and cannot be reached through MCP.
- Tests prove setup/reconcile retains existing ready-workspace state in mocked/provider-fixture paths.

**Required live Apple evidence — hard gate**

On a real supported Apple Silicon Mac:

1. provision Local from a clean state;
2. inspect machine configuration and prove the macOS home is not mounted;
3. as `zodex-agent`, prove `/workspace` is writable and publisher/tunnel secrets are unreadable;
4. prove public Internet egress to at least GitHub/package infrastructure works;
5. prove macOS host services/gateway and the reachable private LAN are denied by a host/provider-controlled boundary;
6. create an agent-owned persistence sentinel and/or small repository/build artifact, stop the machine through the provider, boot via `local exec`, and prove it remains;
7. run `zodex local exec -- sudo ...` with a harmless privileged command/package-query smoke and prove operator administration works while agent sudo does not.

If item 5 cannot be enforced/proven with the current provider architecture, mark Phase 2 **blocked**, update the progress ledger, and add a Plan Amendment before later access phases. Do not mark setup complete with a guest-only firewall workaround.

**Regression evidence**

- Sprite generated setup/upgrade tests still prove the current runtime-only install path and existing verification.
- Direct process-mode identity tests remain valid.
- Existing reader/publisher tests remain green.

#### What must not break

- `zodex-agent`/`zodex-publisher` ownership and publisher key isolation.
- Sprite setup/upgrade scripts and provider lifecycle ownership.
- Runtime installer behavior for existing Linux/Sprite users.
- `/workspace` persistence across ordinary Local lifecycle.
- No-host-mount and private-LAN security acceptance boundary.
- No model-facing privileged Local tool.

### Phase 3 — Secure MCP ingress and durable TTL start/stop lifecycle

#### Files to read before starting

**Agent/MCP runtime contract**

- `src/server.rs` — `ZodexMcpService`, `build_app`, `run_server`; preserve exactly three tool registrations and understand MCP query-key/HTTP listeners.
- `src/protocol.rs` — read in full; no target/provider field is allowed.
- `src/service.rs` — service dispatch reused unchanged.
- `src/session/mod.rs` — `SessionManager`, session handle lifecycle, termination; understand why machine stop invalidates sessions.
- `tests/cli_parity.rs` and representative `src/session/tests.rs` regions around running-session/kill/concurrency behavior.

**Local provider/setup state**

- Current Local provider/setup/exec/status modules from Phases 1–2 — read the lifecycle and ready-state symbols in full.
- Sweep sections **Current data / event / control flow**, **Concurrency / ordering / recovery**, and Landmines 9–12, 16.

**External tunnel facts**

- Re-check the official Secure MCP Tunnel Help Center and `openai/tunnel-client` connector/permissions/release docs from Planning Basis.
- Confirm the current stable Linux ARM64 asset, Streamable HTTP connector configuration, restricted runtime-key requirements and readiness behavior before pinning/installing a supported tunnel client.

#### What to do

Complete the Local access lifecycle with `zodex local start --ttl`, `zodex local stop` and full access/status reporting.

Extend Local setup/reconcile to install a **pinned reviewed stable** Linux ARM64 tunnel client and a provider-owned tunnel configuration/service. Do not fetch an unversioned “latest” executable on every start. Store the OpenAI runtime key in a guest secret file readable only by `zodex-tunnel`; the tunnel configuration references the file rather than embedding the key in argv/logs. The `zodex-agent` account must not be able to read or control that secret/service directly.

Configure the tunnel connector to the existing guest-local Zodex MCP endpoint. Keep `zodexd`'s tool surface and auth unchanged.

`local start` must require `--ttl`. Its ordered behavior is:

1. reconcile any expired/stale previous lease before exposing anything;
2. boot/reconcile the machine and provider-private `zodexd`/`zodex-prd` services;
3. re-check no-host-mount/network/security preconditions;
4. verify guest MCP health locally;
5. start the tunnel service and prove readiness;
6. atomically persist a new generation `LocalAccessLease` with absolute expiry;
7. arm a launchd-supervised host lease worker/reconciler that remains authoritative after the CLI exits;
8. print nonsecret status/endpoint identity and expiry.

Do not automatically restore tunnel access at machine/Mac boot. A new explicit `local start` is required.

Use one shared stop/reconcile operation for manual stop and expiry. It must stop tunnel ingress before stopping the machine, then clear/mark the lease inactive. Handle partial errors truthfully: if tunnel stop succeeds but machine stop fails, report the machine failure while preserving the stronger “MCP inaccessible” state and leaving enough durable state for later reconciliation.

The lease worker must be generation-aware. A renewed `local start` cannot be stopped by a stale worker from an older lease. It must reconcile after Mac sleep and worker process restart. Avoid detached shell `sleep`/`nohup` processes as the authority.

`local status` now reports setup state, machine state, guest service/tunnel health, active/inactive/expired lease and exact expiration when relevant. It must distinguish “machine running because operator exec used it” from “ChatGPT access active.”

#### Validation strategy

**Positive deterministic evidence**

- Tests prove start refuses missing TTL/unsafe setup state and accepts shared duration syntax.
- Injected-clock/provider tests prove lease generation, renewal, expiry, stale-worker rejection, manual stop, partial-stop recovery and post-sleep/late reconciliation.
- Tests prove machine boot/operator exec alone never starts the tunnel.
- Tests prove stop ordering revokes tunnel before machine stop and status truthfully represents intermediate failures.
- Tests prove tunnel secret material is not serialized into Local host records or emitted by status/error rendering.
- MCP contract snapshot/registration tests prove exactly the existing three tools and schemas/annotations remain unchanged.

**Required live evidence**

- Start a Local access window with a short safe TTL and connect ChatGPT through the dedicated Local tunnel/app identity.
- Through MCP, run `id`/`pwd`, a long-running command followed by `write_stdin` polling, and a targeted patch; prove identity/path/output parity with Sprite expectations.
- Manually stop while a long-running process exists; prove the endpoint becomes unavailable and the pre-stop session handle cannot resume after the next start.
- Run a short TTL to actual expiry; prove the tunnel becomes unreachable and the machine stops without the initiating CLI process remaining open.
- Exercise renewal before expiry and prove the prior lease's expiry cannot stop the renewed access.
- If practical, let the Mac sleep across a short expiration and prove wake/reconciliation leaves access revoked rather than extending the lease accidentally.

**Regression evidence**

- Existing MCP/HTTP/CLI parity and session concurrency/timeout tests remain green.
- Sprite's proxy-backed MCP route and server query-key authentication remain unchanged.

#### What must not break

- Exact model-facing tool names/schemas/descriptions/annotations.
- `zodex-agent` authority boundary.
- No automatic Local MCP access from boot or `local exec`.
- Persistent `/workspace` across stop/expiry.
- Manual stop and expiry sharing one authority/order.
- Stable separation between Sprite and Local endpoint identity.

### Phase 4 — GitHub mode target parity with `--local`

#### Files to read before starting

**GitHub policy to preserve**

- `src/bin/zodex/github_mode.rs` — read in full; especially record construction/merge, helper repair/inspection, enable/default/status.
- `src/bin/zodex/dispatch.rs` — `Commands::Github` and `GithubModeCommand` branches.
- `src/bin/zodex/prelude.rs` — `GithubCommand`, `GithubModeCommand`, `ResolvedSprite`, YOLO record types.
- `src/bin/zodex/credentials.rs` — Sprite resolver and shared duration parser.

**Publisher authority**

- `src/bin/zodexd/git_remote.rs` — `handle_git_remote_zodex`, `handle_git_remote_zodex_push`; understand bundle/publisher request path.
- `src/publisher/validation.rs` — `validate_direct_push_request`, `load_active_github_yolo_mode`, `github_mode_allows_repo`.
- `src/publisher/tests.rs` — YOLO scope/expiry and publisher permission tests.

**Target transport pattern**

- `src/bin/zodex/sprite_proxy.rs` — `run_sprite_exec` upload semantics only; do not absorb Sprite lifecycle/proxy behavior.
- Current Local provider root-exec/bounded-transfer primitives from Phases 1–3.
- Sweep Landmines 7, 8, 15, 17 and **Operator-local Sprite registry** section.

**CLI tests**

- `tests/zodex_operator_cli.rs`
- `src/bin/zodex/tests/part2.rs` — registry and YOLO merge/TTL tests.

#### What to do

Refactor only the operator-side guest-manipulation seam required by GitHub mode into the canonical **operator guest target** abstraction. It must support the operations the existing policy genuinely needs: privileged command execution, bounded/atomic state transfer and target identity/status labels.

Keep Sprite's `run_sprite_exec` behavior as one implementation and Local's provider root exec/data transfer as the other. Do not move Sprite service/proxy/setup/lifecycle commands behind this abstraction.

Change YOLO enable/default/status to operate on the abstract selected guest while retaining one `GithubModeRecord` implementation and the existing publisher-side validation unchanged.

Add `--local` to `github mode yolo/default/status`. Keep `--sprite`/`--org` compatible and make explicit Local/Sprite selectors mutually exclusive.

Implement target resolution rules:

- explicit `--local` selects the ready Local target;
- explicit `--sprite` retains existing Sprite behavior;
- if neither is explicit and exactly one eligible target can be inferred, preserve convenience;
- if Local and Sprite are both eligible/plausible, fail closed with guidance to pass `--local` or `--sprite`;
- never write YOLO state to both targets from one command.

Do not have `github mode --local` implicitly call `local start`, extend the Local access TTL or otherwise couple GitHub authority to MCP/machine access. It may use operator provider exec to modify the stopped/running Local machine as required by the provider, but it must not enable the tunnel.

Scope this phase to the **user-approved GitHub mode** Local selector. Do not broaden every grant/list/revoke command merely for symmetry unless current code proves a shared refactor is required to keep one correct path; any such scope consequence must be recorded in the ledger and remain behavior-compatible.

#### Validation strategy

**Positive deterministic evidence**

- CLI tests prove `github mode yolo/default/status --local` and mutual exclusion with `--sprite`.
- Target resolver tests cover: Local-only inference, Sprite-only existing inference, explicit selection, no targets, and Local+Sprite ambiguity.
- Shared target tests prove mode JSON/agent Git repair is installed equivalently through Sprite and Local transports without duplicated policy.
- Existing merge/expiry tests prove repo TTL behavior remains unchanged.
- Tests prove GitHub mode operations do not alter the Local access lease/tunnel state.

**Live evidence**

- On a configured Local target, enable repo-scoped YOLO, verify `status --local`, then return to default and verify only YOLO state is removed.
- With an explicitly authorized disposable GitHub branch/ref, prove a normal agent `git push` succeeds while scoped Local YOLO is active and fails once default/expiry closes it.
- With Sprite and Local both configured, prove explicit mode operations affect only the named target and an omitted selector fails as ambiguous.

If no disposable GitHub write target is explicitly authorized, record that precise live-evidence gap rather than pushing to an arbitrary repository/ref.

**Regression evidence**

- Existing Sprite `--sprite` mode flows and CLI help remain valid.
- Publisher direct-push validation/reader-vs-publisher secret isolation remains unchanged.
- Agent-side `zodex-agent github request-push/publish-pr` behavior remains available.

#### What must not break

- One canonical YOLO policy/merge/expiry implementation.
- Existing Sprite target inference when no Local target creates ambiguity.
- Existing publisher-side enforcement at actual push time.
- Independence between Local MCP access lease and GitHub write authority.
- No host `gh`, SSH agent or Git credential forwarding.

### Phase 5 — Reset, reprovisioning and lifecycle recovery hardening

#### Files to read before starting

**Current Local state/lifecycle**

- Current Local target/provider/setup/lease modules from Phases 1–4 — read the setup transaction, ready state, stop/reconcile and resource symbols in full.
- Current Local tests around interrupted setup, stop/expiry and resource reporting.
- Sweep sections **Persistence and identity**, **Concurrency / ordering / recovery**, and Landmines 9, 10, 12, 16.

**Existing recovery patterns**

- `src/bin/zodex/sprite_proxy.rs` — `sprite_upgrade`, `sync_sprite_services`; learn how current provider repair distinguishes service recreation from workspace destruction.
- `src/bin/zodex/lifecycle.rs` and `src/bin/zodex/process.rs` — stale PID/restart handling patterns at the seam only.
- `src/bin/zodex/status.rs` — current stale/inactive status rendering patterns.

**Provider facts**

- Re-check Apple `container machine rm`, create and resource set/inspect behavior from current command docs/installed version.

#### What to do

Implement the destructive `zodex local reset` contract and close lifecycle/recovery states that are easy to leave ambiguous after the happy path exists.

Reset must load the last ready setup intent and **preflight everything necessary for recreation before deletion**:

- required Apple provider capability/image build inputs;
- reader/publisher source PEM paths;
- tunnel runtime-key source file/reference and tunnel identity;
- resource overrides;
- any other nonsecret setup reference introduced by Amendments.

If any required source is missing/unreadable, fail without stopping/destroying an otherwise healthy machine unless the user separately requested `stop`.

After preflight, reuse the same stop/revoke and setup/reconcile paths rather than adding a second reset-only bootstrap. Delete the Apple machine and persistent storage, clear obsolete provisioning/lease state, then provision a new target from saved intent. Reset success must mean old `/workspace` content is gone and the new target again satisfies setup security checks. ChatGPT access remains **off** after reset; the user runs `local start --ttl ...` explicitly.

Harden setup and status around:

- machine exists but operator record is missing/incomplete;
- record says ready but provider machine is missing;
- partial guest provisioning/service failure;
- tunnel process dies during an active lease;
- machine is externally stopped during a lease;
- stale lease worker/state after manual provider actions;
- repeated setup/start/stop/reset commands;
- requested CPU/memory changes on an existing machine.

Prefer truthful repairable states over automatic destructive recovery. Setup can reconcile Zodex-owned guest runtime without deleting agent workspace; reset remains the only command whose contract authorizes workspace destruction.

#### Validation strategy

**Positive deterministic evidence**

- Injected provider/failure tests cover preflight-before-delete, deletion/recreate failure, interrupted setup records, missing machine, external stop, dead tunnel and stale lease recovery.
- Tests prove a missing secret source prevents reset before provider deletion.
- Tests prove reset reuses the canonical setup path and leaves access inactive.
- Tests prove setup repair does not erase existing `/workspace`, while reset does.
- Tests prove optional CPU/memory intent is preserved/reapplied and status reflects drift where provider inspection differs.

**Live evidence**

- Create a clear pre-reset sentinel/repo/build cache, run reset with valid inputs, and prove the old state is gone while guest identities/Git/tunnel isolation are re-established.
- Temporarily make a reprovisioning source unavailable, run reset, and prove the original machine/storage was not deleted.
- Externally stop the machine during an active short lease and prove `local status`/next `stop`/`start` reconciles rather than reporting false active access.
- Re-run setup on a machine containing user-installed development packages and workspace data and prove those survive reconciliation.

**Regression evidence**

- Manual stop/TTL still preserve persistence.
- GitHub `--local` mode remains independent and target resolution remains fail-closed.
- Sprite recovery/service sync remains untouched.

#### What must not break

- Reset is the sole ordinary command authorized to destroy Local workspace persistence.
- No destructive reset before all recreation inputs are proven available.
- Access stays off after reset until explicit start.
- Setup remains non-destructive/idempotent for a ready machine.
- Host/network/secret isolation is re-proven after recreation.

### Phase 6 — Public docs, packaging and Sprite-preserving product integration

#### Files to read before starting

**Current reader-facing product story**

- `README.md` — current Sprite-first overview and install/setup path.
- `src/content/docs/architecture.md` — runtime/component story.
- `src/content/docs/quickstart.md` — current end-to-end Sprite setup and credentials.
- `src/content/docs/command-reference.md` — public command map.
- `src/content/docs/configuration.md` — guest runtime config vs operator state explanations.
- `src/content/docs/write-modes.md` — current GitHub autonomy UX.
- `src/content/docs/proxy-mcp.md` — Sprite-specific ingress; Local tunnel material must not blur this page's provider scope.
- `src/content/docs/tools.md` — exact model-facing surface that must remain provider-neutral.
- `src/content/docs/development.md` — validation/public binary expectations.
- `src/data/docs.ts` — current global Sprite-backed docs description.

**Packaging/install**

- `.github/workflows/release.yml` — Apple/Linux release matrix.
- `scripts/install.sh` — operator Darwin install and runtime Linux install.
- `tests/install_script.rs`, `tests/binary_manifest.rs`, `tests/zodex_operator_cli.rs`.
- Current Local CLI help/output tests from prior phases.

**Regression orientation**

- Sweep **Sprite compatibility requirement** evidence via **Current user-visible behavior**, **External / provider / platform boundaries**, and Landmine 15.

#### What to do

Integrate Local into the supported public product story without turning Sprite into legacy or presenting a generic provider abstraction to users.

Add task-oriented Local documentation that explains:

- Apple Silicon/macOS/container-machine prerequisites;
- one-time `local setup` and required GitHub/tunnel inputs;
- optional CPU/memory sizing;
- `local exec` as operator-only environment administration;
- persistent `/workspace`/caches and the no-host-mount boundary;
- `local start --ttl`, `local stop`, truthful status and `local reset`;
- separate `Zodex Local` vs `Zodex Sprite` ChatGPT MCP app identities;
- Internet egress vs host/private-LAN isolation;
- `github mode --local` as a separate permission from Local machine access;
- recovery/troubleshooting for unsupported provider, interrupted setup, expired access and missing reprovisioning inputs.

Update shared architecture/command/write-mode docs so they describe both peer targets accurately. Keep Sprite proxy/service/org material under Sprite-specific pages and examples. Do not rename/remove working Sprite commands merely to make the docs symmetrical.

Ensure `zodex --help`, `zodex local --help`, Local subcommand help and GitHub mode help are sufficient to discover the supported workflow without the planning documents.

Verify packaging remains one Zodex operator distribution. If Local image/tunnel metadata/assets are compile-time embedded, make release/package tests prove the installed macOS binary can create required temporary setup assets without repository-relative files. Add only packaging changes actually required by the implemented design.

Do not edit the changelog solely as implementation diary; follow `AGENTS.md` release-note rules when an actual release is cut.

#### Validation strategy

**Positive evidence**

- CLI help tests cover the complete final Local command surface and `github mode --local`.
- Docs build/check proves all added pages/navigation/references resolve.
- A clean macOS operator installation from the release artifact can run `zodex local status` and reconstruct any embedded Local setup assets without a repo checkout.
- Task-oriented docs are reviewed against the actual current CLI output, not planned names.

**Regression evidence**

- Existing Sprite docs/examples still point to working Sprite commands/proxy behavior.
- Existing release matrix still produces Linux and both macOS targets.
- Installer tests still prove runtime-only and operator-only paths.
- MCP tools docs still list exactly the same three tools.

#### What must not break

- Sprite remains described and supported as a first-class target.
- No planning/phase terminology leaks into product docs.
- No secret examples contain real credential material.
- One operator install/release path remains sufficient.
- Local docs do not imply host-folder access, agent root or automatic GitHub permission.

### Phase 7 — Independent end-to-end acceptance, security and performance hardening

#### Files to read before starting

**Acceptance contract**

- This implementation plan — read in full including all Amendments and all 46 acceptance criteria.
- `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-spec-2026-08-13.md` — re-check approved product/security decisions.
- `gg/local-apple-silicon-runtime/local-apple-silicon-runtime-progress.md` — read Current handoff and all workstream entries, but treat claims as evidence to verify rather than proof.

**Current implementation**

- Inspect all current files changed by the workstream via Git history/diff; do not rely on the original planning baseline paths if modules moved.
- Re-open `src/server.rs`, `src/protocol.rs`, publisher validation/direct-push code, current Local provider/setup/lease/target modules, Sprite provider code, release/install code and final public docs at their current symbols.
- Re-open representative deterministic tests for every major acceptance cluster.

**Research risks**

- Sweep Landmines 1–18 and factual gaps 1–5.
- Re-check current Apple Container and OpenAI tunnel primary documentation/version notes before live acceptance.

#### What to do

Perform an independent Crucible-style acceptance/hardening pass against the **current code**, not the progress ledger. Map every acceptance criterion to actual deterministic/live evidence and close missing defect-class regressions before declaring the workstream complete.

On a supported Apple Silicon Mac, exercise the product from a clean or safely reset state through the real operator CLI and real provider. Acceptance must include:

- clean setup with no macOS home/project mounts;
- agent/publisher/tunnel OS identity and secret-read barriers;
- public Internet allowed while macOS/private LAN denied;
- operator `local exec` administration without agent root;
- persistent workspace/cache behavior across stop/start;
- Secure MCP Tunnel connection through the actual Zodex Local ChatGPT app;
- exact three-tool schema/behavior parity, including long-running session polling and patching;
- manual stop and actual TTL expiry, renewal and stale-session behavior;
- interrupted/stale lifecycle recovery;
- reset preflight and destructive recreation;
- `github mode --local` scope/status/default and live push on an explicitly authorized disposable ref when available;
- simultaneous Sprite and Local target identity with no cross-routing;
- representative large Rust build using the configured Local resources and reuse of build artifacts after stop/start.

Run the repository's broad validation gate and any docs/package checks introduced by the implementation. Review the final source for leaked keys, PEMs, tunnel configuration secrets, local host paths/private test data and temporary security probes. Remove temporary migration/compatibility paths that are not part of the one intended final architecture.

Record a workstream-local acceptance matrix/evidence summary if it materially helps map all 46 criteria. A criterion dependent on unavailable provider/authorized live state must be recorded truthfully as unproven rather than silently marked PASS.

#### Validation strategy

**Positive evidence**

- Every acceptance criterion has explicit current deterministic or live evidence, or a truthful unresolved/unsupported result that matches a Plan Amendment.
- `bash scripts/check.sh` passes on the final branch.
- Relevant docs build/check and macOS release/install smoke pass.
- Security probes demonstrate the actual host boundary rather than only inspecting configuration strings.
- Representative Rust build/persistence evidence proves the motivating large-build workflow without asserting an invented benchmark target.

**Regression evidence**

- Exercise at least one real existing Sprite connection/workflow after Local is fully configured, including command execution and an operator Sprite status/mode action, to prove Local did not become the only practical path.
- Re-run existing Sprite setup/script/service/proxy tests, MCP parity tests, publisher policy tests and macOS/Linux packaging tests.

#### What must not break

- Any of the accepted security invariants while closing acceptance gaps.
- Sprite as a simultaneous first-class execution target.
- Exact MCP schema/tool identity.
- GitHub authority independence and publisher-side enforcement.
- Persistent Local workspace outside explicit reset.
- No private live evidence or secret material committed to the repository.

## Amendments

None yet.
