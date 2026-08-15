# Phase 7 Apple Silicon Acceptance Matrix

Date: 2026-08-14

Target: the designated Apple Silicon integration Mac with Apple Container 1.2.2 and the dedicated Zodex Local tunnel. Secret values, private-key contents, private host paths and disposable repository names are intentionally omitted.

Status vocabulary:

- `PASS`: current deterministic and/or live evidence satisfies the criterion.
- `PARTIAL`: substantial current evidence exists, but a required manual or disruptive slice remains.
- `UNPROVEN`: the required live action has not been authorized or performed.

| # | Status | Current evidence |
| ---: | --- | --- |
| 1 | PASS | A release-mode macOS operator binary copied outside the checkout completed status and setup using only embedded assets. |
| 2 | PASS | Provider/platform/version/capability fixture tests pass and live Apple Container 1.2.2 provisioning fails closed on observed provider errors. |
| 3 | PASS | Live machine inspection reported `homeMount: none`; setup and start reverified isolation. |
| 4 | PASS | Live workspace, agent home, repositories, caches and build artifacts were created on persistent guest storage. |
| 5 | PASS | Sentinels, Git state, installed packages, Rust toolchain/cache and release artifact survived stop/start and setup reconciliation. |
| 6 | PASS | Live daemon/descendant identity and namespace probes showed unprivileged `zodex-agent`; sudo, capability and namespace/network-administration attempts failed. |
| 7 | PASS | Live publisher service ran as `zodex-publisher`; agent reads of publisher credentials failed. |
| 8 | PASS | Live tunnel service ran as `zodex-tunnel` in the restricted namespace; agent reads of runtime key and MCP bearer failed. |
| 9 | PASS | Live operator exec ran in the root namespace and installed OS packages; equivalent model-facing privilege probes failed. |
| 10 | PASS | Operator exec booted a stopped machine, preserved data and left tunnel/MCP inactive. |
| 11 | PASS | Public DNS, GitHub HTTPS and package-registry access succeeded from the agent namespace. |
| 12 | PASS | Live probes denied the namespace gateway, vmnet/macOS, private/LAN, link-local, reserved, multicast and IPv6 destinations; policy is interface-scoped, root-owned, restart-reconciled and unavailable to the agent. |
| 13 | PASS | Daemon remained loopback-only in the guest; inbound access used the separate outbound tunnel. |
| 14 | PASS | CLI and duration tests require an explicit finite TTL and accept the shared `s/m/h/d` grammar. |
| 15 | PASS | Live start repaired runtime/isolation, connected the tunnel, recorded an expiry and printed no secret. |
| 16 | PASS | Live status independently reported machine, service, tunnel and MCP lease state. |
| 17 | PASS | Live stop revoked access, stopped the machine, invalidated sessions and preserved guest storage. |
| 18 | PASS | A short live TTL expired after the initiating CLI exited; launchd supervision revoked the tunnel and stopped the machine. |
| 19 | PARTIAL | Live autonomous expiry and launchd worker recovery passed, including the corrected launchd PATH. Actual Mac sleep across expiry remains unperformed. |
| 20 | UNPROVEN | A real Mac reboot during a nominal lease has not been authorized or performed. |
| 21 | PASS | Live renewal replaced the lease generation and the older worker did not revoke the renewed access window. |
| 22 | PASS | Repeated stop, stale lease, externally stopped machine and partial setup states reconciled truthfully and idempotently. |
| 23 | PASS | A live pre-stop session handle returned unknown after restart and was not resumable. |
| 24 | PASS | Current MCP schema tests and live model-facing HTTP calls exercised exactly `exec_command`, `write_stdin` and `apply_patch` without a target field. |
| 25 | PARTIAL | The actual Local model-facing service performed workspace inspection, long-running command polling and targeted patching. The same workflow through the ChatGPT app UI remains unperformed. |
| 26 | PARTIAL | Local and Sprite remained simultaneously configured; explicit routing and live Sprite execution passed with no Local/Sprite mutation crossover. Simultaneous ChatGPT app identities remain unverified in the UI. |
| 27 | PASS | Starting Local did not grant direct push; push required an independent Local GitHub mode change. |
| 28 | PASS | Live repo-scoped Local YOLO with a finite TTL installed the shared policy. |
| 29 | PASS | Live Local status reported scope/readiness; Local default removed only Local YOLO state. |
| 30 | PASS | Live explicit Sprite status and agent execution passed after Local integration; pre-existing Sprite grants were preserved. |
| 31 | PASS | Live ambiguous target omission failed closed; explicit Local and Sprite selection behaved independently. |
| 32 | PASS | Existing single-target inference tests pass without introducing global mutable selection. |
| 33 | PASS | Live clone/fetch/push used the existing GitHub App/helper path; host credentials and SSH authority were not forwarded. Publisher and reader isolation tests pass. |
| 34 | PASS | Live normal push to the authorized disposable private repository succeeded while repo-scoped Local YOLO was active and failed after Local default mode. |
| 35 | PASS | Live status covered configuration, recovery, provider, machine, resources, home mount, services, isolation, tunnel, exec and MCP access without secrets. |
| 36 | PASS | Repeated live setup preserved sentinels, installed packages, repositories, Rust cache and artifacts. |
| 37 | PASS | Several interrupted provisioning states were reported as incomplete and repaired by a later setup without state-file editing. |
| 38 | PASS | Making the saved tunnel key unavailable caused reset to fail before stopping/deleting the healthy machine; restoring it allowed recovery. |
| 39 | PASS | Live reset revoked access, destroyed persistent data, reprovisioned identities/network/services and left MCP inactive. |
| 40 | PASS | Live CPU/memory reconciliation changed the target to 3 CPUs/3 GiB, reported it in status and retained it through reset intent. |
| 41 | PASS | Current stable Rust built a release ripgrep checkout in the agent namespace; stop/start preserved the toolchain, cache and artifact and an incremental rebuild reused them. |
| 42 | PASS | State/log/argv inspection and final tracked-diff scan found no raw PEM, runtime key, API key, private test path or credential material. |
| 43 | PASS | `cargo build --release --bin zodex` and the source-checkout-independent release artifact smoke passed on Apple Silicon. |
| 44 | PASS | Live Sprite agent execution/status passed and the complete Sprite/operator regression suites pass. |
| 45 | PASS | Current Local docs/help cover setup, exec, lifecycle, identity, resources, persistence, isolation, GitHub mode and reset. Astro check/build pass. |
| 46 | PASS | `bash scripts/check.sh`, docs check/build, release artifact setup smoke, source-size guard and private-material scan pass on the final automated-test tree. |

## Deferred live actions

Phase 7 remains `in_progress` until all four manual/disruptive facts are proved:

1. connect the dedicated Zodex Local app in ChatGPT and exercise the three-tool workflow;
2. keep Sprite and Local ChatGPT identities configured simultaneously and prove no cross-routing;
3. sleep the Mac past a short Local TTL and verify wake remains revoked;
4. reboot the Mac while a nominal lease remains and verify Local MCP does not automatically return.

The first two require the user's authenticated ChatGPT UI. The last two interrupt the user's Mac and require explicit approval. No current implementation or security-boundary defect is known from the completed automated and live CLI slices.
