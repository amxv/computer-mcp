# GitHub App Auth For Agent PR Workflows

This repository includes a GitHub App based workflow for giving an agent the minimum GitHub access it needs:

- create a branch
- push commits to that branch
- open a pull request

The goal is to avoid placing a broad personal access token on a VPS.

## What Is Manual

GitHub App registration is still a manual GitHub step.

GitHub's supported setup flow for a private GitHub App is documented in the GitHub UI and docs. Use a private app and install it only on the target repository.

One-time manual steps:

1. Register a private GitHub App under your personal account or org.
2. Grant only these repository permissions:
   - `Contents: Read & write`
   - `Pull requests: Read & write`
3. Install the app on the single target repository.
4. Generate and download the app private key (`.pem`).
5. Record:
   - `GITHUB_APP_ID`
   - `GITHUB_APP_INSTALLATION_ID`
   - `GITHUB_APP_PRIVATE_KEY_PATH`

## Included Scripts

### Mint an installation token

```bash
GITHUB_APP_ID=123456 \
GITHUB_APP_INSTALLATION_ID=789012 \
GITHUB_APP_PRIVATE_KEY_PATH=/secure/path/app.private-key.pem \
./scripts/mint-gh-app-installation-token.sh
```

By default this prints only the short-lived installation token.

### Protect main

Before giving the repo to an agent, protect the default branch server-side:

```bash
./scripts/protect-main-branch.sh --repo OWNER/REPO --branch main --approvals 1
```

This enforces the important part of the model:

- the agent can push feature branches
- the agent opens PRs
- direct pushes to `main` are blocked by GitHub

For a private repository, GitHub only enables protected branches with GitHub Pro, GitHub Team,
GitHub Enterprise Cloud, or GitHub Enterprise Server. On a private personal-account repository
without GitHub Pro, this command will fail with a GitHub plan error and `main` will not be
protected server-side.

If you want server-side blocking on a private repo, use one of these:

- upgrade the owner account to GitHub Pro
- move the repository to an organization on GitHub Team or Enterprise
- make the repository public

Without that plan support, the GitHub App still gives the agent scoped branch push and PR access,
but preventing direct pushes to `main` becomes a local-policy problem instead of a GitHub-enforced
rule.

### Create branch, push, and open PR

From a checkout of the target repository:

```bash
GITHUB_APP_ID=123456 \
GITHUB_APP_INSTALLATION_ID=789012 \
GITHUB_APP_PRIVATE_KEY_PATH=/secure/path/app.private-key.pem \
./scripts/agent-create-pr.sh \
  --repo OWNER/REPO \
  --branch agent/example-change \
  --title "Agent: example change" \
  --body "Automated change from computer-mcp agent." \
  --commit-message "agent: implement example change"
```

Optional author identity:

```bash
./scripts/agent-create-pr.sh \
  --repo OWNER/REPO \
  --branch agent/example-change \
  --title "Agent: example change" \
  --body "Automated change from computer-mcp agent." \
  --commit-message "agent: implement example change" \
  --name "computer-mcp-bot" \
  --email "bot@users.noreply.github.com"
```

## Recommended Storage On VPS

Store only:

- `GITHUB_APP_ID`
- `GITHUB_APP_INSTALLATION_ID`
- the GitHub App private key file

Do not store a broad personal access token.

Recommended file permissions:

```bash
chmod 600 /secure/path/app.private-key.pem
```

## Notes

- Installation tokens are short-lived, which reduces blast radius compared to a broad PAT.
- The scripts here do not merge PRs. They stop at branch push + PR creation.
- Branch protection is the real safety control. Prompt instructions are not enough on their own.
