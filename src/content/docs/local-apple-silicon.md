---
title: Zodex Local on Apple Silicon
description: Set up a persistent isolated Linux coding machine on an Apple Silicon Mac, connect it to ChatGPT through Secure MCP Tunnel, size it, operate it, and recover it safely.
order: 1.5
category: Start
summary: "The end-to-end guide for Zodex Local: Apple Container prerequisites, setup inputs, persistent workspace behavior, TTL access, isolation, GitHub policy, reset, and recovery."
---

Zodex Local gives ChatGPT the same three Zodex tools as a Sprite, but runs the coding computer in a persistent Linux machine on your Apple Silicon Mac.

Use Local when you want Mac CPU, RAM, and local-storage performance with a Linux-native workspace that survives between sessions. Keep using [Zodex Sprite](/docs/quickstart) when you want a remote machine that is independent of your Mac. Both can stay configured at the same time.

## What Local creates

Local owns one fixed Apple Container machine named `zodex-local`. Its workspace and caches live on the machine's persistent Linux storage; Zodex does **not** mount your macOS home directory into it.

The normal agent workspace is:

```text
/workspace/
  repos/
    my-repo/
```

Stopping and starting Local preserves repositories, `.git` state, language/package caches, `target/`, `node_modules`, installed packages, and other files inside the Linux machine. `zodex local reset` is the explicit destructive exception.

## Requirements

Before setup, have:

- an Apple Silicon Mac
- the Apple Container CLI installed and able to start its machine service
- the `zodex` operator CLI installed on the Mac
- a Local Linux machine environment capable of systemd plus Linux network namespaces, veth pairs, and nftables; the embedded Zodex Local image installs the required `iproute2`/`nftables` tooling and setup fails closed if that boundary cannot be created
- a reader GitHub App with `Contents: Read-only`
- a publisher GitHub App with `Contents: Read & write` and `Pull requests: Read & write`
- private-key PEM files for those two apps stored on the operator Mac
- a dedicated pre-created OpenAI Secure MCP Tunnel ID and a restricted file containing its runtime key

The reader and publisher app permissions are the same as for Sprite. See [GitHub Apps setup](/docs/github-apps) for the permission model.

The tunnel ID and runtime-key file are setup inputs, not values Zodex creates on your behalf. Keep the runtime-key file private. Zodex installs the key into the isolated Linux machine with restricted ownership and does not put it in process arguments.

## Install the operator CLI

The same operator distribution manages Sprite and Local:

```bash
curl -fsSL https://zodex.ashray.xyz/install.sh | sh
zodex --version
```

On Apple Silicon, the installer selects the macOS ARM64 release artifact. You do not need a Zodex repository checkout to create or repair the Local machine; the operator binary carries the Local machine and service templates it needs.

## Create or reconcile Local

Run setup from the Mac:

```bash
zodex local setup \
  --repo owner/repo \
  --reader-app-id <reader-app-id> \
  --reader-pem /secure/zodex/reader.pem \
  --publisher-app-id <publisher-app-id> \
  --publisher-pem /secure/zodex/publisher.pem \
  --tunnel-id tunnel_<32-lowercase-hex> \
  --tunnel-runtime-key /secure/zodex/local-tunnel-runtime-key \
  --cpus 8 \
  --memory 16G
```

`--cpus` and `--memory` are optional. Omit them to use Apple Container defaults. Re-running setup is the normal non-destructive reconciliation path: it preserves the Local persistent disk while repairing owned runtime/network state and applying new resource intent. A CPU or memory change can require the machine to stop before the new allocation takes effect.

Setup records nonsecret source references such as PEM/key file paths so reset can preflight a known-good recreation later. Keep those source files available if you want `zodex local reset` to be able to recreate the machine without re-entering setup arguments.

## Install OS-level development tools

`local exec` is the operator-only privileged maintenance path:

```bash
zodex local exec -- apt-get update
zodex local exec -- apt-get install -y clang mold pkg-config
```

It runs as guest root outside the model-facing restricted network namespace. It is deliberately not an MCP tool and is not available to ChatGPT. Use it for one-time OS packages, machine inspection, or trusted repair work; let the agent do normal coding as the unprivileged `zodex-agent` user.

## Check status before opening access

```bash
zodex local status
```

Status is read-only. It reports the saved setup state, provider and machine state, observed CPU/memory, home-mount isolation, guest-service health, network/isolation verification, tunnel state, reset-recovery readiness, and MCP lease state.

Treat drift messages as repair signals rather than permission to ignore the boundary. Re-run `local setup` with the original inputs when setup or owned network state needs reconciliation.

## Open ChatGPT access for a finite TTL

Local access always requires an explicit finite TTL:

```bash
zodex local start --ttl 2d
```

Start boots/prepares the machine, verifies the model-facing isolation boundary and services, starts the Secure MCP Tunnel, waits for readiness, then persists the access lease and arms host-side expiry supervision. If those steps cannot complete safely, the command fails closed instead of publishing an active lease.

When the TTL expires, Zodex revokes tunnel access and stops the Local machine while preserving its persistent disk. A stale earlier expiry worker cannot revoke a newer renewed lease.

To revoke access immediately:

```bash
zodex local stop
```

You can later reopen the same machine with another finite `local start --ttl ...`.

## Keep a separate ChatGPT identity

Use two stable MCP app/configuration identities when you keep both targets:

```text
Zodex Sprite
Zodex Local
```

Bind `Zodex Local` to the dedicated Secure MCP Tunnel used during Local setup. Do not point an old Sprite conversation at Local or add a `target` argument to Zodex tools. Both endpoints intentionally expose exactly:

```text
exec_command
write_stdin
apply_patch
```

The endpoint identity selects the computer; the tool protocol stays the same. This also lets Local and Sprite serve different conversations at the same time.

## Understand the Local isolation boundary

Model-facing commands run as the unprivileged `zodex-agent` user inside a root-owned Linux network namespace. Zodex puts `zodexd`, `zodex-prd`, the tunnel process, and their model-launched descendants inside that same restricted network boundary.

The intended default is:

| Destination or authority | Model-facing Local access |
| --- | --- |
| Public IPv4 Internet, GitHub, package registries | Allowed |
| macOS filesystem and home directory | Not mounted |
| macOS localhost/services | Denied by the Local network boundary |
| Private/LAN, link-local, reserved and multicast IPv4 | Denied |
| IPv6 from the model-facing namespace | Disabled |
| macOS Keychain, SSH agent, host Git credentials | Not forwarded |
| Linux root/network administration | Not granted to `zodex-agent` |

The trusted boundary is important: Zodex Local isolates the **unprivileged coding agent** from macOS and private-network authority. It trusts the Apple Container Linux kernel and the root-owned Local control plane. It does not claim containment after a guest-kernel exploit or compromise of trusted guest root.

## GitHub write authority is separate

Starting Local makes the MCP computer reachable; it does **not** grant direct GitHub push permission. Likewise, enabling GitHub YOLO mode does not start the Local tunnel.

For a trusted repo-scoped session:

```bash
zodex github mode yolo --local --repo owner/repo --ttl 2d
zodex github mode status --local
```

Return only the GitHub policy to default with:

```bash
zodex github mode default --local
```

Use `zodex local stop` separately when you also want to close MCP access. If both Sprite and Local are configured, prefer explicit `--local` or `--sprite <name>` selection for operator GitHub mode commands; ambiguous target inference fails closed rather than granting both.

Review-first `zodex-agent github publish-pr`, agent-requested grants, normal commits, and reader-backed clone/fetch work the same way inside Local as they do on Sprite.

## Reset the machine completely

`reset` is intentionally destructive:

```bash
zodex local reset
```

It permanently erases the Local machine's persistent storage and recreates the machine from the last known-good setup intent. Existing repositories, caches, packages, build outputs, and other Local-only files are lost.

Before deletion, reset checks the saved network/image identity, required local source files, GitHub App authority, tunnel input and machine-image build path. If a required PEM or tunnel runtime-key source is missing or unreadable, reset refuses before destroying the existing machine.

After successful reset, MCP access stays **off**. Open a new finite access window explicitly with `zodex local start --ttl ...`.

## Recovery patterns

### Setup was interrupted

Re-run the same `zodex local setup ...` command. Setup is the non-destructive repair path and does not erase the workspace. If a previously ready machine had begun a later interrupted setup, Zodex also retains its separate last-ready reset intent so recovery does not require editing host state by hand.

### The machine was stopped outside Zodex

Run:

```bash
zodex local status
zodex local stop
```

`status` will not call a lease active when the machine/runtime/isolation is unavailable. `stop` reconciles stale access state fail-closed. Start a new lease afterward with `zodex local start --ttl ...`.

### The tunnel died or a lease expired

Check `zodex local status`. A valid-looking lease is not enough for status to report active MCP access if the tunnel, daemon or isolation checks fail. `local stop` is always the explicit revoke path; a later `local start --ttl ...` reconciles stale/expired lease state before opening a new window.

### Network/isolation drift is reported

Re-run `local setup` with the saved setup inputs. Zodex stops model-facing services before its one network reconciler changes the namespace/veth/nftables state and restarts services only after the boundary verifies.

### Reset says a saved source is unavailable

Restore the referenced PEM/runtime-key file or intentionally run setup again with replacement source paths. Do not work around the reset preflight by manually deleting `zodex-local`; the preflight exists to keep a healthy persistent machine from being erased when it cannot be recreated safely.

## Day-to-day Local loop

```bash
# Operator Mac
zodex local status
zodex local start --ttl 2d
zodex github mode yolo --local --repo owner/repo --ttl 2d

# In ChatGPT: use the Zodex Local app/configuration.
# The agent works normally under /workspace/repos.

# Operator Mac when finished
zodex github mode default --local
zodex local stop
```

The next session reuses the same Linux filesystem. Use reset only when you actually want a clean machine.
