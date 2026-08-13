# zodex

`zodex` is a ChatGPT-native coding workspace with two first-class Linux execution targets:

- **Zodex Sprite** — a remote Sprites.dev machine
- **Zodex Local** — a persistent isolated Linux machine on an Apple Silicon Mac

Both give ChatGPT the same tiny MCP tool surface that GPT models already know how to use well:

- `exec_command`
- `write_stdin`
- `apply_patch`

ChatGPT can clone repos, inspect code, edit files, run tests, keep long-lived sessions alive, and commit inside the selected Linux target. The operator decides how GitHub writes happen:

- PR-only publishing without direct shell write tokens
- one-off repo-scoped push approval from the Sprite
- remote operator-granted push windows
- timed YOLO mode for trusted sessions
- repo-scoped YOLO for selected repos
- no-TTL YOLO for intentionally trusted environments

The supported repository slug for this project is `amxv/zodex`.

## Why it exists

ChatGPT coding works best when the model has familiar tools and a real machine. zodex gives it both:

1. a real Linux workspace through either Sprite or Local instead of a simulated sandbox
2. command/stdin/patch tools that fit GPT coding behavior
3. normal Git history and normal test commands
4. operator-controlled GitHub write autonomy

Sprites are a good fit when you want a remote machine that stays independent of your Mac. Local is a good fit when you want Apple Silicon CPU/RAM and local-storage performance while keeping the agent inside its own persistent Linux filesystem instead of mounting your macOS development folders. You can keep both configured and choose the computer per conversation.

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

# The same operator GitHub policy can target Zodex Local explicitly.
zodex github mode yolo --local --repo owner/repo --ttl 4h
zodex github mode status --local
zodex github mode default --local
```

`mode yolo` defaults to a `2h` TTL and all installed repositories. Passing `--repo` changes the scope to a repo allowlist; repeated repo-scoped YOLO commands merge with active repo grants instead of replacing them, and each repo keeps its own TTL. Passing `--no-ttl` makes the new window indefinite until the operator disables it. `mode default` removes only YOLO state and leaves explicit push grants alone.

## Choose a setup path

For a remote Sprite, see the [Quickstart](src/content/docs/quickstart.md). The shape is:

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

For an Apple Silicon Mac, see [Zodex Local on Apple Silicon](src/content/docs/local-apple-silicon.md). Local uses the same `zodex` operator install, creates a persistent `zodex-local` Linux machine with no macOS home mount, and opens ChatGPT access only through an explicit finite TTL:

```bash
zodex local setup \
  --repo owner/repo \
  --reader-app-id <reader-app-id> \
  --reader-pem /absolute/path/to/reader.pem \
  --publisher-app-id <writer-app-id> \
  --publisher-pem /absolute/path/to/writer.pem \
  --tunnel-id tunnel_<32-lowercase-hex> \
  --tunnel-runtime-key /absolute/path/to/local-tunnel-runtime-key \
  --cpus 8 \
  --memory 16G

zodex local start --ttl 2d
```

Keep separate ChatGPT MCP identities such as `Zodex Sprite` and `Zodex Local`. They intentionally expose the same `exec_command`, `write_stdin`, and `apply_patch` tools; the endpoint identity selects the computer.

## Core commands

```bash
zodex sprite status --sprite zodex-dev
zodex sprite logs --sprite zodex-dev --service zodexd --lines 100
zodex sprite sync --sprite zodex-dev --force-recreate
zodex sprite upgrade --sprite zodex-dev
zodex local status
zodex local exec -- apt-get install -y clang mold pkg-config
zodex local start --ttl 2d
zodex local stop
zodex local reset
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

This repository includes an Astro documentation site for zodex. It covers ChatGPT setup for Sprite and Local, Local isolation/lifecycle, Sprite runtime architecture, GitHub App access, write modes, proxy and MCP front door, direct HTTP API, command reference, troubleshooting, and docs maintenance.

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
