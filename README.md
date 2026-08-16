# zodex

`zodex` puts ChatGPT on real coding machines through a tiny, familiar MCP surface.

It has two supported execution modes:

- **Sprite** — a remote Sprite-backed Linux workspace with isolated GitHub read/write policy.
- **Local** — trusted direct execution on an Apple Silicon Mac, with one runtime shared by many ChatGPT conversations plus Agent-aware history and live observability.

Both modes expose the same three tools GPT models already know how to use well:

- `exec_command`
- `write_stdin`
- `apply_patch`

ChatGPT can inspect code, edit files, run tests, and keep long-lived sessions alive. Sprite mode additionally provides the repository GitHub access/write-control model described below:

- PR-only publishing without direct shell write tokens
- one-off repo-scoped push approval from the Sprite
- remote operator-granted push windows
- timed YOLO mode for trusted sessions
- repo-scoped YOLO for selected repos
- no-TTL YOLO for intentionally trusted environments

The supported repository slug for this project is `amxv/zodex`.

## Why it exists

ChatGPT coding works best when the model has familiar tools and a real machine. zodex gives it both:

1. real machine execution instead of a simulated coding sandbox
2. command/stdin/patch tools that fit GPT coding behavior
3. a remote Linux option for isolated/persistent workspace work
4. a trusted direct-Mac option when the host itself is the intended workspace
5. normal Git history and normal test commands
6. operator-controlled GitHub write autonomy in Sprite deployments

Sprites are a good fit when you want remote Linux. Local is a good fit when the code, toolchains, credentials, and files you deliberately want ChatGPT to use already live on a trusted Apple Silicon Mac.

## Write modes

Start safe, then open more autonomy when the session earns it.

### Review-first PR

```bash
zodex-agent github publish-pr \
  --repo owner/repo \
  --title "Title" \
  --base main \
  --body "Summary and tests."
```

`publish-pr` bundles the current committed `HEAD`, sends it to the local publisher daemon, and lets that daemon push a generated branch and open a PR. The writer-app token stays inside `zodex-prd` instead of being exposed to the agent shell.

### One-off push approval

```bash
zodex-agent github request-push --repo owner/repo
# then normal Git works
git push origin main
zodex-agent github revoke-push --repo owner/repo
```

The default active grant TTL is `30m`. Change it with `--ttl 2h`, disable TTL enforcement with `--no-ttl`, and opt into refresh-token caching with `--cache-refresh-token` only when intended.

### Operator-granted push

```bash
zodex github grant-push --sprite dev-sprite --repo owner/repo
git push origin main
zodex github revoke-push --sprite dev-sprite --repo owner/repo
```

Use this when the human operator should open the write window from their own machine.

### YOLO mode

```bash
zodex github mode yolo --sprite dev-sprite
zodex github mode yolo --sprite dev-sprite --ttl 4h
zodex github mode yolo --sprite dev-sprite --repo owner/repo
zodex github mode yolo --sprite dev-sprite --no-ttl
zodex github mode status --sprite dev-sprite
zodex github mode default --sprite dev-sprite
```

`mode yolo` defaults to a `2h` TTL and all installed repositories. Passing `--repo` changes the scope to a repo allowlist; repeated repo-scoped YOLO commands merge with active repo grants instead of replacing them, and each repo keeps its own TTL. Passing `--no-ttl` makes the new window indefinite until the operator disables it. `mode default` removes only YOLO state and leaves explicit push grants alone.

## Choose a setup path

Install the `zodex` operator CLI in either case:

```bash
curl -fsSL https://zodex.ashray.xyz/install.sh | sh
zodex --version
```

### Zodex Local

On an Apple Silicon Mac, provision an existing OpenAI Secure MCP Tunnel and start from the workspace you want published to ChatGPT as the suggested initial explicit workdir:

```bash
zodex local setup
cd ~/code/owner/repo
zodex local start --ttl 4h
zodex local status
```

Every `exec_command` and `apply_patch` call still declares an absolute workdir; the start directory is guidance, not a backend cwd fallback. Inspect concurrent conversations with:

```bash
zodex local watch
zodex local history --agent k7m2 --since 1h
zodex local stop
```

See [Zodex Local](https://zodex.ashray.xyz/docs/local) for the trusted-host model, one runtime-wide TTL, Agent correlation, history, macOS privacy boundary, and process-cleanup scope.

### Sprite mode

See the [Sprite quickstart](https://zodex.ashray.xyz/docs/quickstart) for the no-clone remote-Linux path. The setup flow is:

1. install the local `zodex` operator CLI
2. install and authenticate the Sprite CLI
3. create and select a Sprite
4. make the Sprite URL public for ChatGPT MCP access
5. create the reader and writer GitHub Apps
6. run `zodex sprite setup`
7. connect ChatGPT to the `/mcp?key=...` URL

Create two GitHub Apps:

- reader app: `Contents: Read-only`
- writer app: `Contents: Read & write`, `Pull requests: Read & write`, Device Flow enabled, user access token expiration enabled

Install zodex on the Sprite:

```bash
zodex sprite setup \
  --sprite zodex-dev \
  --repo owner/repo \
  --reader-app-id <reader-app-id> \
  --reader-pem /absolute/path/to/reader.pem \
  --publisher-app-id <writer-app-id> \
  --publisher-pem /absolute/path/to/writer.pem \
  --default-base main \
  --url-auth sprite
```

Connect ChatGPT to:

```text
https://<sprite-host>/mcp?key=<zodex-api-key>
```

## Core commands

```bash
zodex local setup
zodex local start ~/code/owner/repo --ttl 4h
zodex local status --json
zodex local watch --agent k7m2
zodex local history --agent k7m2 --since 1h
zodex local stop
zodex sprite status --sprite zodex-dev
zodex sprite logs --sprite zodex-dev --service zodexd --lines 100
zodex sprite sync --sprite zodex-dev --force-recreate
zodex sprite upgrade --sprite zodex-dev
zodex proxy inspect --sprite zodex-dev
zodex proxy verify-origin --sprite zodex-dev
zodex-agent github publish-pr --repo owner/repo --title "Title"
zodex-agent github request-push --repo owner/repo
zodex github grant-push --sprite zodex-dev --repo owner/repo
zodex github mode yolo --sprite zodex-dev --repo owner/repo --ttl 4h
zodex github mode default --sprite zodex-dev
zodex-agent show-url --host <public-host>
```

## Documentation site

This repository includes an Astro documentation site for zodex. It covers Zodex Local, Local observer/dashboard clients, Sprite setup and runtime architecture, GitHub App access, write modes, MCP tooling, direct HTTP APIs, command reference, troubleshooting, and docs maintenance.

Run it locally with:

```bash
bun install
bun run dev
```

Validate the docs site with:

```bash
bun run check
bun run build
```

Deploy the docs worker with:

```bash
bun run deploy:docs
```

Production routing keeps `zodex.ashray.xyz` on the existing Cloudflare proxy worker. That worker forwards `/mcp`, `/mcp/*`, and `/health` to the live Sprite and sends all other paths to the `zodex-docs` worker origin.

The Astro docs content lives in `src/content/docs`, with site-wide navigation and metadata in `src/data/docs.ts`.
