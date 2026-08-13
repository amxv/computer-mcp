# Local Apple Silicon Runtime — Current-State Sweep

## Coordinates and scope

- **Repository:** `amxv/zodex`
- **Checkout:** `/Users/ashray/code/amxv/zodex`
- **Branch inspected:** `main`
- **Original planning baseline:** `6f35f14866842b6608887937825680231f90bf46`
- **Current implementation coordinate:** `7028bcaa48705a045dfc1b8e47af68a55c27c1a1`
- **Sweep date:** 2026-08-13
- **Requested capability basis:** [`local-apple-silicon-runtime-spec-2026-08-13.md`](./local-apple-silicon-runtime-spec-2026-08-13.md)
- **Planning workflow:** user-supplied Existing Repository Feature Planning Workflow, read in full before this sweep.

This sweep maps the implementation relevant to adding an Apple Silicon Local execution target while preserving Sprite behavior. It now includes the completed Phase 1 surface, the hidden Phase 2 foundation at `7028bcaa48705a045dfc1b8e47af68a55c27c1a1`, and live Apple Container 1.2.2 network findings that constrain the remaining implementation. The implementation plan remains the authoritative phase sequence.

Repository Markdown outside the explicitly supplied specification and required repository instructions was not used as planning-history evidence. Current product documentation under `src/content/docs/` was read as part of the public contract; old neighboring `gg/` planning artifacts were deliberately not consumed.

## Architecture map

The package currently builds six named binaries, although the product documentation often groups them into five operational roles:

```text
operator machine
┌─────────────────────────────────────────────────────────────┐
│ zodex                                                       │
│  ├─ direct Linux service lifecycle                         │
│  ├─ Sprite setup / services / proxy operations             │
│  ├─ truthful read-only Local status                         │
│  ├─ hidden Local provider/setup/state foundation            │
│  └─ operator-side GitHub grant / YOLO controls             │
│                                                             │
│ zodex-client                                                │
│  └─ direct HTTP API client/debug surface                    │
└─────────────────────────────────────────────────────────────┘
                          │
                          │ Sprite CLI / HTTPS / GitHub APIs
                          ▼
Sprite guest
┌─────────────────────────────────────────────────────────────┐
│ zodexd                                                      │
│  ├─ MCP: exec_command / write_stdin / apply_patch           │
│  └─ /v1 JSON HTTP equivalents                              │
│                                                             │
│ zodex-agent                                                 │
│  ├─ restricted GitHub helper surface                        │
│  ├─ git credential helper                                   │
│  └─ forwards hidden operations to zodexd                    │
│                                                             │
│ git-remote-zodex                                            │
│  └─ intercepts normal Git pushes                            │
│                                                             │
│ zodex-prd                                                   │
│  └─ publisher/direct-push broker with writer credentials    │
│                                                             │
│ /workspace                                                  │
│  └─ persistent agent-owned workspace                        │
└─────────────────────────────────────────────────────────────┘

Apple Container Local machine, foundation present but setup not public
┌─────────────────────────────────────────────────────────────┐
│ persistent Ubuntu/systemd machine, home mount disabled      │
│  ├─ provider-root command/data-transfer seam                │
│  ├─ zodex-agent / zodex-publisher / zodex-tunnel identities │
│  ├─ loopback daemon and provider-private service assets     │
│  └─ fail-closed placeholder network-isolation gate          │
└─────────────────────────────────────────────────────────────┘
```

### Operator CLI module shape

`src/bin/zodex.rs` is only a wrapper around `src/bin/zodex/mod.rs`. The operator implementation is split by responsibility through `include!` modules:

- `src/bin/zodex/prelude.rs` — imports, constants, Clap command/data types.
- `src/bin/zodex/dispatch.rs` — top-level command dispatch.
- `src/bin/zodex/credentials.rs` — GitHub credential parsing, shared TTL parsing, Sprite registry and Sprite resolution.
- `src/bin/zodex/sprite_proxy.rs` — Sprite CLI/API transport, setup, upgrade, service sync, public proxy helpers and Sprite verification.
- `src/bin/zodex/github_device.rs` — GitHub device-flow grants and operator/agent grant operations.
- `src/bin/zodex/github_mode.rs` — YOLO state construction/merge, Sprite-side installation, status and disable.
- `src/bin/zodex/lifecycle.rs` — direct Linux install/start/stop/restart/TLS lifecycle.
- `src/bin/zodex/process.rs` — non-systemd process-mode daemons and ownership.
- `src/bin/zodex/status.rs` — direct/Sprite status rendering, Sprite service definitions, direct systemd unit rendering.
- `src/bin/zodex/system_tls.rs` — TLS setup helpers.
- `src/bin/zodex/local_provider.rs` — Apple provider capability parsing, machine/image lifecycle, literal-argv root exec and bounded stdin-based transfer.
- `src/bin/zodex/local_state.rs` — Local target/setup-source/lease records and atomic host-state persistence.
- `src/bin/zodex/local_setup.rs` — hidden setup/reconcile transaction, embedded guest provisioning and post-provision verification; its network gate is deliberately fail closed until the selected namespace boundary is implemented.
- `src/bin/zodex/local_machine.Containerfile`, `local_zodexd.service`, `local_zodex_prd.service` — embedded Ubuntu/systemd image and provider-private guest service assets.

The module split is recent organizational work, but the underlying operator responsibilities predate it.

### Agent tool/service shape

`src/server.rs` creates `ZodexMcpService` around the transport-independent `ZodexService` in `src/service.rs`. MCP dispatch goes through the same service layer as the direct JSON HTTP API.

The MCP server registers exactly:

```text
exec_command
write_stdin
apply_patch
```

`src/protocol.rs` owns their request/response schemas. `src/session/mod.rs` owns PTY process/session behavior. `src/apply_patch.rs` owns Codex-style patch rewriting/application.

There is no Sprite identity or provider identity in the model-facing tool inputs. That is an important current property: the daemon executes against the computer on which it is running.

## Current user-visible behavior

### Operator command tree

`Commands` in `src/bin/zodex/prelude.rs` currently exposes:

```text
install
upgrade
start
stop
restart
status
logs
set-key
rotate-key
git-credential-helper
show-url
tls
sprite
proxy
github
local
```

`local` currently exposes only truthful read-only `status`. Hidden setup/provider code exists, but `local setup` and `local exec` are intentionally absent from the public Clap tree until the remaining Phase 2 vertical slice is safe and complete.

`SpriteCommand` exposes:

```text
setup
upgrade
sync
status (services-status alias)
logs (service-logs alias)
health
```

There is no public `zodex sprite exec` command in current `main`. Internally, however, `run_sprite_exec` is the central privileged guest-control primitive, and current product docs show raw `sprite exec -- sudo ...` for operator inspection.

### Direct Linux service commands

Top-level `zodex install/start/stop/restart/status/logs/tls` are direct Linux service-management commands. `ensure_linux()` rejects those service-management paths on macOS.

The installer itself already distinguishes an **operator install** from a **runtime install**:

- operator mode supports Darwin and Linux;
- runtime mode is Linux-only and creates service identities/runtime files.

This matters because the macOS `zodex` binary already exists as a supported operator binary even though it cannot directly host `zodexd` service management.

### Phase 1 and hidden Phase 2 foundation

Phase 1 is complete. The public `zodex local status` path detects platform/provider support, inspects the fixed `zodex-local` machine identity and distinguishes not-configured, provider-ready/missing, machine state and inactive MCP access without mutating host or guest state. Provider output parsing and CLI help/status behavior have deterministic coverage.

Commit `7028bcaa48705a045dfc1b8e47af68a55c27c1a1` also landed a coherent but deliberately hidden Phase 2 foundation:

- an embedded Ubuntu 24.04/systemd machine recipe and provider-private service assets;
- explicit no-home-mount machine create/resource reconciliation;
- provider-root execution that preserves argv boundaries plus bounded stdin-based file transfer;
- durable nonsecret setup-source records and ready/provisioning validation;
- runtime-installer-based guest provisioning for `/workspace`, identities, Git helpers and TLS;
- loopback daemon configuration, explicit agent/publisher service users and a separate restricted tunnel identity;
- post-provision checks and a fail-closed placeholder network-isolation gate.

The next implementation must extend these responsibilities. It must not rebuild them as a second Local provider or expose partial public commands around the placeholder gate.

### Sprite setup

`zodex sprite setup` accepts a Sprite identity plus a seed repository, reader/writer GitHub App IDs and PEM paths, default base and Sprite URL auth. It resolves installation IDs on the operator side, proves the supplied GitHub App credentials can mint tokens, then runs a generated setup script inside the Sprite.

The generated setup script:

1. ensures basic guest packages such as Git/curl/certificates;
2. downloads the canonical Zodex installer;
3. invokes `ZODEX_INSTALL_MODE=runtime` with `ZODEX_INSTALL_OPERATOR_CLI=0`;
4. fixes the guest runtime to `/home/zodex-agent` and `/workspace`;
5. installs reader and publisher keys with different ownership/modes;
6. writes GitHub App installation config;
7. configures the agent Git credential helper, push rewrite and identity;
8. proves `/workspace` is writable as the agent;
9. proves reader-backed Git access works;
10. proves the agent cannot read the publisher private key;
11. leaves service supervision to the Sprite Services control plane.

After the script, the operator synchronizes Sprite Services and verifies health, logs, socket permissions, Git behavior and secret separation.

### MCP and HTTP ingress

`zodexd` serves:

- `/health` without authentication;
- `/mcp` and `/mcp/` behind query-key authentication;
- `/v1/exec-command`, `/v1/write-stdin`, `/v1/apply-patch` behind Bearer authentication.

`run_server` always requires TLS certificate/key material for its TLS listener. An optional `http_bind_port` adds a plain HTTP listener serving the same Axum application.

Sprite deployments currently configure TLS on `8443` and plain HTTP on `8080`; the public Sprite/proxy layer handles external reachability.

## Relevant subsystems and ownership

### `zodex-agent`

The runtime installer creates `zodex-agent` as a system user with:

- primary group `zodex`;
- home `/home/zodex-agent`;
- login shell `/bin/bash` when available;
- ownership of its home and `/workspace`.

The product docs explicitly state that the agent should not run as root.

In Sprite Services, the expected `zodexd` command is:

```text
sudo -n -u zodex-agent /usr/local/bin/zodexd --config /etc/zodex/config.toml
```

In process mode, `daemon_launch_command` likewise uses `runuser -u <agent_user>` whenever the lifecycle command is running as root.

### `zodex-publisher`

The installer creates `zodex-publisher` as a separate system user with:

- the shared `zodex` group;
- no useful home;
- a nologin shell.

The publisher private key is installed `0600` owned by `zodex-publisher`. The publisher socket directory is `0750` and the socket is `0660`; the shared group allows the agent-side Git helper to submit requests without reading the publisher key.

The Sprite setup verification explicitly fails if `zodex-agent` can read `/etc/zodex/publisher/private-key.pem`.

### `/workspace`

`Config::default()` sets `default_workdir = "/workspace"`. Runtime installation creates it with agent ownership. Process-mode lifecycle also creates/re-owns it.

There is no repository registry inside the runtime. Repositories are ordinary directories under the agent-owned filesystem.

### Operator-local Sprite registry

`credentials.rs` stores Sprite operator metadata at:

```text
~/.config/zodex/sprites.json
```

`OperatorSpriteRecord` contains:

```text
name
org
remote_config
last_setup_at
```

The registry does not contain guest API keys or GitHub PEM content.

## Current data / event / control flow

### MCP command flow

```text
ChatGPT
  │ MCP /mcp?key=...
  ▼
ZodexMcpService (src/server.rs)
  │ ServiceRequest
  ▼
ZodexService (src/service.rs)
  │
  ├─ SessionManager::exec_command
  ├─ SessionManager::write_stdin
  └─ apply_patch::apply_patch
```

`exec_command` creates `/bin/bash -lc <cmd>` under the daemon's OS identity, attaches a PTY, tracks output in memory and returns after the requested yield or process exit. Long-running work returns an eight-character session handle.

`write_stdin` looks up the in-memory `SessionRuntime`; no session state is persisted to disk. A daemon/machine restart therefore invalidates old handles by construction.

### Sprite operator flow

```text
zodex operator CLI
  │
  ├─ run Sprite CLI (`sprite exec`, `sprite url`)
  ├─ call Sprite control API through `sprite api`
  ├─ call GitHub APIs locally for setup/grant flows
  └─ upload setup/grant state into guest
        │
        ▼
     Sprite guest
```

`run_sprite_exec` accepts command tokens and optional local-file uploads. Several GitHub operator commands depend directly on this function instead of an abstract guest-control interface.

### Direct push flow

Normal Git configuration rewrites GitHub push URLs to the `zodex::` remote helper:

```text
git push
  ▼
git-remote-zodex
  ├─ determine owner/repo
  ├─ create Git bundle for pushed ref
  └─ DirectPushRequest over Unix socket
        ▼
      zodex-prd
        ├─ load YOLO policy
        ├─ validate repo/ref/bundle
        ├─ mint short-lived writer installation token
        └─ push requested ref
```

The agent shell does not receive the publisher installation private key.

### YOLO flow

Operator-side `github mode yolo` builds a `GithubModeRecord`, merges still-active per-repo grants, then writes `/var/lib/zodex/mode/state.json` through a Sprite privileged exec. The record supports:

- all-installed scope;
- repo allowlist scope;
- per-repo expiration;
- no-TTL mode.

The publisher independently reads that state and checks whether the requested repo is allowed at request time.

## Persistence and identity

### Persistent guest state

Important current guest persistence is ordinary filesystem state:

- `/workspace` — code/build work;
- `/home/zodex-agent` — agent-local Git/tool state;
- `/etc/zodex/config.toml` — runtime configuration;
- `/etc/zodex/reader/private-key.pem` — reader key;
- `/etc/zodex/publisher/private-key.pem` — publisher key;
- `/var/lib/zodex/push-grants` — direct push grants;
- `/var/lib/zodex/mode/state.json` — YOLO policy;
- `/var/lib/zodex/publisher` — publisher runtime/socket/log state;
- `/var/lib/zodex/tls` — daemon TLS material.

Sprite persistence itself is supplied by Sprites.dev rather than encoded in Zodex.

### Session identity

MCP process sessions are intentionally ephemeral and daemon-local. Handles are random eight-character alphanumerics kept in an in-memory map. The manager also has an internal numeric session ID used for logging/output, but returned session continuity is keyed by the handle.

### Sprite target identity

The operator selects Sprite target identity through, in priority order:

1. an explicit `--sprite` argument;
2. `ZODEX_SPRITE`;
3. exactly one candidate in the operator Sprite registry.

Zero candidates or multiple candidates fail rather than choosing arbitrarily. This is already a fail-closed target-resolution pattern.

## External / provider / platform boundaries

### Sprites.dev

Current Sprite behavior depends on two distinct provider surfaces:

- the `sprite` CLI for exec, URL operations and uploads;
- the Sprite API for managed service definitions/status/logs.

Sprite Services are treated as lifecycle authority once the guest is recognized as a Sprite (`/.sprite`). Direct guest `zodex start` refuses to create a competing detached lifecycle when Sprite Services are not already supervising the expected processes.

### Apple container/container-machine — current external facts

Apple documentation describes `container machine` as a persistent Linux environment whose filesystem survives stop/start. Apple Container 1.2.2 on the implementation Mac exposes the machine creation, start/stop, run/exec, inspect, set and removal shapes used by the Phase 1/hidden Phase 2 foundation.

The current default is security-relevant: container-machine creation can mount the macOS user's home read/write unless home mounting is disabled. The CLI supports disabling the home mount explicitly.

Current Apple machine docs also describe `container machine run` as booting a stopped machine when necessary, and expose privileged/root execution for operator control. Machine removal deletes the persistent machine state.

Apple's normal container network documentation supports isolated user-defined networks and an `--internal` mode that prevents public Internet access. Apple Container 1.2.2 does not expose a machine network selector for the inverse combination “public Internet allowed, host/private LAN denied.” Its built-in shared NAT therefore cannot itself be treated as the agent network boundary.

Primary references inspected on 2026-08-13:

- `https://github.com/apple/container/blob/main/docs/container-machine.md`
- `https://github.com/apple/container/blob/main/docs/command-reference.md`
- `https://github.com/apple/container/blob/main/docs/how-to.md`
- `https://github.com/apple/container/blob/main/docs/technical-overview.md`
- `https://github.com/apple/container/blob/main/docs/network.md`

### Live Apple Container 1.2.2 network findings

Live disposable probes on the implementation Mac established the following:

- the built-in machine network used `192.168.64.0/24` and gave the guest public GitHub HTTPS plus reachability to its `192.168.64.1` vmnet gateway, the native Mac and the physical private-LAN gateway;
- an exact-source PF anchor on `bridge100` could preserve public IPv4/DNS/HTTPS while denying tested private destinations, and did not affect a second Apple container;
- guest root with `NET_ADMIN` added another source address in the built-in subnet and bypassed that exact-source PF rule;
- matching the full bridge/subnet would affect unrelated Apple containers, and PF is not a supported distributed Apple Container policy API.

Those findings rule out per-address PF, whole-shared-network PF and an unrestricted provider interface as the final model-facing boundary. They do not require replacing Apple Container. The selected V1 design instead trusts the Linux machine kernel/root control plane and places all model-facing services and descendants in one root-owned network namespace with only loopback plus a dedicated veth. Interface-scoped nftables in the trusted root namespace drops input arriving from that veth, permits/NATs only public IPv4 forwarding, denies non-public IPv4 and all IPv6, and uses public IPv4 DNS. Operator `zodex local exec` remains in the trusted root namespace. The unprivileged `zodex-agent` receives no sudo, network-administration, system-administration or namespace-control capability.

This is a deliberately narrower threat model than hostile guest root or kernel containment. The design protects the workspace host/private network from the unprivileged coding agent while preserving Apple Container performance and persistence.

### OpenAI Secure MCP Tunnel — current external facts

Current OpenAI guidance says ChatGPT cannot directly connect to a localhost-only MCP server and documents Secure MCP Tunnel for developer/private-network MCP servers.

The open-source tunnel client is an outbound client process that forwards a configured local MCP server through the OpenAI control plane. Current release assets include Linux ARM64, so there is no platform requirement that the tunnel client itself run on macOS.

Current tunnel documentation recommends restricted runtime credentials for tunnel use instead of an administrator key, and supports secret references such as file-backed secrets.

Primary references inspected on 2026-08-13:

- `https://help.openai.com/en/articles/12584461-developer-mode-and-full-mcp-connectors-in-chatgpt-beta`
- `https://github.com/openai/tunnel-client`
- `https://github.com/openai/tunnel-client/blob/master/docs/connectors.md`
- `https://github.com/openai/tunnel-client/blob/master/docs/permissions.md`
- `https://github.com/openai/tunnel-client/releases`

## Authentication and secret handling

### MCP API key

`zodexd` compares the MCP `key` query parameter with `Config.api_key`. The direct JSON API uses the same configured API key as a Bearer token.

CLI status/log helpers use `redact_api_key_query_params` to avoid printing query secrets in normal diagnostics.

### Reader App

The reader GitHub App private key is currently placed under `/etc/zodex/reader`. The shared-service model allows `zodex-agent` to call the credential helper, which mints short-lived read installation tokens on demand.

The reader App permission profile is read-only contents access.

### Publisher App

The publisher private key is separated by OS ownership. `zodex-prd` mints short-lived publisher installation tokens only after validating the publish/direct-push request.

### Push grant state

`PushGrantRecord` contains an actual temporary token plus expiry. Sprite operator grant installation writes it into `/var/lib/zodex/push-grants` with controlled permissions. Agent-requested device-flow cache is separate under the agent's home and is optional.

### Operator-side inputs

Sprite setup receives PEM paths as operator inputs and uploads their contents only into the targeted guest. The Sprite registry stores target metadata, not PEM content.

## Concurrency / ordering / recovery

### Command sessions

`SessionManager` uses:

- a `RwLock<HashMap<...>>` for session lookup;
- a per-session operation mutex to serialize stdin/poll operations;
- process groups for SIGTERM/SIGKILL;
- bounded output buffers;
- idle timeout and forced-kill grace period.

Tests cover unrelated-session concurrency, repeated poll/write behavior, kill and timeout behavior, handle isolation and working-directory reporting.

### Service restart

A daemon restart intentionally loses in-memory session handles. There is no persisted resume protocol.

### Sprite lifecycle recovery

`sync_sprite_services` can delete/recreate provider service definitions. Sprite upgrade verifies service logs, health, agent Git identity, reader access, socket permissions and publisher-key isolation after replacement.

### YOLO merge/expiry

Operator-side YOLO records preserve active per-repo TTLs when another repo is added. Expired grants are pruned during merge. Publisher-side validation re-checks expiration using epoch seconds rather than trusting operator status output.

There is no analogous current durable timer or lease supervisor for machine/MCP access. The only time-based authority currently in Zodex is checked lazily when GitHub policy is read/used.

## Public API / CLI / UI / schema / generated contracts

### MCP contract

`src/protocol.rs` contains no deployment target field. Current tool descriptions/annotations are registered in `src/server.rs` and product docs deliberately advertise exactly three tools.

### Operator CLI contract

`tests/zodex_operator_cli.rs` currently checks GitHub mode help and expects `--sprite`, `--repo`, `--ttl`, `--no-ttl`.

`src/content/docs/command-reference.md` documents Sprite commands and direct service commands. It currently says that when exactly one Sprite is registered, `--sprite` can be omitted for remote operator commands.

### Product documentation contract

The current docs still position Sprites.dev as the primary deployment target. Important pages for this work are:

- `src/content/docs/architecture.md`
- `src/content/docs/quickstart.md`
- `src/content/docs/command-reference.md`
- `src/content/docs/configuration.md`
- `src/content/docs/write-modes.md`
- `src/content/docs/proxy-mcp.md`
- `src/content/docs/tools.md`
- `src/content/docs/development.md`
- top-level `README.md`

`src/data/docs.ts` also currently describes Zodex as Sprite-backed.

### Packaging contract

`.github/workflows/release.yml` already publishes:

```text
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
aarch64-apple-darwin
x86_64-apple-darwin
```

The macOS archives include the operator CLI. `scripts/install.sh::detect_operator_platform` supports Apple Silicon Darwin and installs a non-root operator CLI to `~/.local/bin` when `/usr/local/bin` is unavailable.

The Local feature therefore does not begin from a Linux-only operator distribution.

## Existing tests and what they prove

### `src/bin/zodex/tests/part1.rs`

Representative coverage includes:

- Clap requirements around Sprite setup;
- rendered direct systemd unit shape;
- direct status rendering and service manager detection;
- Sprite expected service definitions;
- publisher status and run user;
- Sprite API command construction;
- process/service inspection helpers.

The systemd-unit test only verifies `ExecStart`, restart policy and sections. It does **not** assert the runtime identity.

### `src/bin/zodex/tests/part2.rs`

Representative coverage includes:

- Apple provider command construction/inspect fixtures and capability classification;
- Local host-state load/save/readiness and no-home-mount behavior;
- hidden Local setup/provider service and transfer foundations;
- YOLO default scope and TTL;
- no-TTL and repo scoping;
- expiration cutoff;
- merge behavior with independent per-repo TTLs;
- Git helper repair/inspection;
- Sprite registry explicit/env/single/ambiguous resolution;
- shared TTL parsing for seconds/minutes/hours/days;
- push-grant expiry.

### `src/publisher/tests.rs`

Covers publisher request formats, YOLO repo/expiry checks, reader/publisher permission profiles, request-size bounds and publish-target selection.

### `tests/cli_parity.rs`

Proves service/HTTP/CLI behavior remains consistent for command execution, stdin and patch paths. This is useful evidence that the computer service itself is deployment-independent.

### `tests/sprite_scripts.rs`

Checks generated Sprite setup/upgrade script properties and protects existing Sprite behavior.

### `tests/install_script.rs`

Checks installer structure, runtime binaries and expected agent-facing guidance.

### `tests/zodex_operator_cli.rs`

Checks the current operator GitHub command/help surface plus the public Local status/help surface.

### `tests/source_file_size.rs`

Enforces a 1000-line ceiling for repository-owned source files. Provider work that simply grows the current include modules can fail this guard even if behavior is correct.

### Canonical repository validation

`AGENTS.md` defines:

```bash
bash scripts/check.sh
```

as the broad validation gate, covering format, clippy, source-file size and the Rust test suite.

## Existing patterns worth copying

### 1. Operator target resolution fails closed

The Sprite registry resolver is explicit when multiple targets are possible rather than selecting the first configured target.

### 2. Guest runtime installation is already separable from provider lifecycle

`ZODEX_INSTALL_MODE=runtime` installs users, binaries, dirs, config, TLS and Git helpers while `ZODEX_INSTALL_OPERATOR_CLI=0` avoids installing a guest operator CLI. Sprite setup then supplies provider-specific service supervision separately.

### 3. Privileged operator transport is distinct from MCP execution

`run_sprite_exec` is an operator-side primitive. MCP execution continues under the agent daemon. That is already the conceptual separation needed for “operator can administer, model cannot.”

### 4. Post-provision verification is substantive

Sprite setup/upgrade do not treat command success as sufficient. They verify service health, Git identity, reader access, socket modes and publisher secret isolation.

### 5. GitHub policy is enforced on the publisher request path

A direct Git push does not succeed merely because the agent's Git config says YOLO is on. The publisher independently validates current state and repo scope.

### 6. macOS operator distribution already exists

Release and install surfaces already understand Apple Silicon, reducing the number of packaging concepts a Local provider needs to add.

## Relevant history

History was inspected only where current behavior was surprising.

- `bf61856` added the dedicated agent workspace model for Sprite deployments.
- `648a57b` added the restricted `zodex-agent` surface.
- `66eea9a` made Sprite guests runtime-only, reinforcing provider/operator vs guest separation.
- `ac9d51f` added YOLO direct Git push.
- `30fc438` evolved installer/setup flow.
- `620ed1f` split the previously large operator CLI into responsibility modules; it preserved existing systemd-unit behavior rather than introducing it.
- The direct systemd unit's lack of `User=`/`Group=` predates the module split. No later history inspected established that running the agent daemon as root is an intentional supported security model.
- `dd5cd84` improved non-root operator installation, confirming macOS/non-root operator usage is a maintained path.
- `7028bcaa` completed the Phase 1 Local status/provider/state seam and added the hidden Phase 2 machine/setup/service foundation while retaining a fail-closed network gate.

## Landmines

### 1. `workdir` is not a filesystem security boundary

`SessionManager::resolve_command_cwd` accepts an arbitrary supplied path. The spawned shell is an ordinary `/bin/bash -lc` process and can access whatever the daemon OS identity can access. `apply_patch` also accepts absolute patch paths. The current safe mental model is therefore **computer/OS isolation**, not path validation.

### 2. Apple container-machine's default home mapping contradicts the requested boundary

A default machine can share the macOS user's home. Merely creating an Apple container machine is insufficient evidence of host isolation. Home mounting must be inspected, not assumed.

### 3. The provider network is not the agent network boundary

Live Apple Container 1.2.2 probes proved that the built-in shared NAT provides public Internet together with macOS/vmnet/private-LAN reachability. Per-address PF is bypassable by guest root changing its source address, while full bridge/subnet PF affects unrelated containers. The selected V1 boundary must therefore be created inside the persistent Linux machine by its trusted root control plane: all model-facing services and descendants join one dedicated namespace with no provider NIC, and interface-scoped root-owned nftables allows only public IPv4 through a veth. The unprivileged agent must be unable to change or leave that topology.

### 4. The direct systemd unit does not specify the agent user

`render_systemd_unit` creates a root-owned system service with no `User=` or `Group=`. In contrast, Sprite Services and process mode explicitly run `zodexd` as `zodex-agent`, and product docs say the agent should not run as root. A normal systemd-based Local guest cannot blindly reuse that direct unit while claiming current Sprite authority parity.

### 5. Runtime-only install deliberately leaves provider lifecycle outside the installer

When `ZODEX_INSTALL_OPERATOR_CLI=0`, `run_runtime_install` does not call the operator CLI's `install` path. This is why Sprite setup can own service supervision separately. A new provider that expects the runtime installer to have enabled its services would be incomplete.

### 6. Current host `--config` semantics do not represent a remote target

Top-level `--config` is a guest/direct-service config path such as `/etc/zodex/config.toml`. The Sprite registry is a separate host-side record. Reusing `/etc/zodex/config.toml` as Local host-target state would conflate two different authorities and break macOS assumptions.

### 7. GitHub mode policy code is target-independent in concept but Sprite-coupled in transport

`enable_github_yolo_mode`, `disable_github_yolo_mode`, `print_github_mode_status` and helper inspection take `ResolvedSprite` and call `run_sprite_exec` directly. Copying these functions for Local would create permanent dual policy implementations.

### 8. Sprite's internal exec primitive supports file upload; Local target operations also need an atomic data-transfer shape

GitHub mode installation currently uploads a generated mode JSON file. A target abstraction limited to “run command” cannot preserve this behavior without either unsafe shell interpolation or a separate data-transfer primitive.

### 9. There is no existing durable machine-access lease supervisor

GitHub TTLs are enforced lazily when grants are read/used. “At TTL expiry stop the tunnel and machine” is an active lifecycle effect that must survive the invoking CLI exiting, process crashes and Mac sleep. Current Zodex has no host-side scheduler/process supervisor abstraction for this.

### 10. Starting the machine and granting ChatGPT access are not the same state

Current Apple `container machine run` can boot a stopped machine to execute an operator command. If MCP ingress were enabled automatically on machine boot, an innocent `zodex local exec` could restore ChatGPT access outside its intended TTL.

### 11. Tunnel authority must not collapse into `zodex-agent`

A Secure MCP Tunnel runtime credential is an authority-bearing secret. If the same unprivileged account that executes model commands can read or restart the tunnel arbitrarily, host-side TTL revocation becomes harder to make authoritative. The tunnel still belongs in the restricted agent namespace because it is model-reachable ingress and must not gain a separate provider-network bypass.

### 12. `zodexd` currently requires TLS artifacts even when a plain HTTP listener is used

The Local transport can consume an internal HTTP listener, but the daemon still refuses to start without the configured TLS cert/key. Runtime setup already creates TLS material, so omitting that step would produce a surprising startup failure.

### 13. Provider-specific code must still compile on the full release/test matrix

The operator CLI is built for Linux and both macOS architectures. Local functionality cannot make the entire `zodex` binary Darwin-only at compile time. Unsupported platforms should fail through runtime capability checks while shared parsing/help/tests remain portable.

### 14. The source-file-size guard makes provider decomposition load-bearing

Several operator modules are already substantial. New provider/state/lease responsibilities need coherent module boundaries; adding everything to `prelude.rs`/`dispatch.rs`/`sprite_proxy.rs` risks a mechanical 1000-line failure and a poor long-term architecture.

### 15. Sprite remains a maintained product path, not migration scaffolding

This work is additive. Sprite setup/service/proxy tests and docs are therefore regression contracts, not old code scheduled for deletion.

### 16. MCP sessions cannot survive a machine stop and should not be represented as resumable

Session handles live only in `zodexd` memory. Any Local stop/expiry/restart necessarily makes pre-stop handles stale. Status/recovery code must not imply otherwise.

### 17. YOLO authority currently lives inside the guest and is sound only because the agent is unprivileged

Publisher validation trusts root/publisher-owned guest mode state. That model remains valid only while model execution cannot rewrite the authoritative mode file.

### 18. The existing duration parser already accepts `d`

`parse_push_grant_ttl` accepts `s`, `m`, `h`, and `d`, including the desired `2d` syntax. Duplicating another subtly different TTL grammar would create needless operator inconsistency.

## Coverage statement

Mapped deeply:

- the operator CLI command/dispatch/module seams;
- Sprite setup, privileged exec, service ownership and verification;
- runtime installer and direct/process lifecycle behavior;
- MCP/HTTP server and session ownership;
- GitHub reader/publisher/direct-push/YOLO flow;
- operator Sprite registry and target inference;
- relevant tests, packaging and user-facing docs;
- current Apple container-machine and OpenAI Secure MCP Tunnel capabilities needed to identify provider boundaries.
- Phase 1 Local status/provider/state plus the hidden Phase 2 setup/service foundation at `7028bcaa`;
- live Apple Container 1.2.2 shared-NAT and PF-bypass evidence, and the resulting namespace trust boundary.

Checked at the seam rather than exhaustively:

- TLS certificate internals, because Local reuses existing runtime TLS generation and the requested change is not a TLS redesign;
- detailed GitHub device-flow HTTP implementation, because Local does not change the OAuth protocol;
- publisher's actual Git fetch/push implementation after request validation, because Local preserves that guest-side architecture;
- docs-site presentation code, because the feature requires content changes rather than a visual redesign.

Deliberately out of scope:

- old `gg/` plans and historical planning narratives;
- arbitrary host-folder sharing;
- macOS-native direct command execution;
- non-Apple-Silicon Local providers;
- menu-bar/UI work.

## Factual gaps / things not proven

1. **Namespace primitives and boot ordering on the final Local image.** The selected root-owned namespace/veth/nftables design is implementation-ready, but the actual `zodex-local` Ubuntu machine has not yet live-proven kernel support, systemd ordering, public-DNS behavior, reboot reconciliation and fail-closed service attachment. Phase 2 must implement and test these contracts deterministically; Phase 7 must prove and repair them live before the workstream is accepted or released.
2. **Future Apple CLI/version drift.** Apple Container 1.2.2 on the implementation Mac matched the implemented machine shapes, but setup must continue to verify capabilities and fail closed after provider upgrades rather than assuming compatibility indefinitely.
3. **Secure MCP Tunnel account provisioning details for this user's account.** The tunnel client and Linux ARM64 runtime are documented, but the actual tunnel ID/runtime key must be operator-supplied and validated live. The plan must not require an admin key to be stored in Zodex.
4. **Whether a disposable GitHub repo/ref is available for automated live Local push validation.** Deterministic publisher tests can prove policy, but end-to-end write validation needs an explicitly authorized disposable target.
5. **Representative Rust-build performance on the user's Mac.** Architecture supports local CPU/RAM and persistent Linux storage, but no performance ratio versus Sprite or native macOS has been measured. Acceptance should prove resource/persistence behavior, not invent a speed target.
