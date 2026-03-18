#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/protect-main-branch.sh [--repo OWNER/REPO] [--branch main] [--approvals 1]

Defaults:
  --branch main
  --approvals 1

Behavior:
  - requires pull requests before merging
  - requires at least one approval
  - dismisses stale approvals
  - requires approval of the most recent push
  - includes admins in enforcement
  - requires linear history
  - blocks force pushes and branch deletions
  - requires conversation resolution

This is intended for the GitHub App workflow where the agent can push feature branches
and open PRs, but cannot push directly to main.
EOF
}

die() {
  echo "$*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

repo=""
branch="main"
approvals="1"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) repo="${2:?missing value for --repo}"; shift 2 ;;
    --branch) branch="${2:?missing value for --branch}"; shift 2 ;;
    --approvals) approvals="${2:?missing value for --approvals}"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
done

require_cmd gh

if [[ -z "${repo}" ]]; then
  repo="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"
fi

set +e
output="$(
  gh api \
    --method PUT \
    -H "Accept: application/vnd.github+json" \
    "/repos/${repo}/branches/${branch}/protection" \
    -f required_status_checks:=null \
    -F enforce_admins=true \
    -F required_pull_request_reviews[dismiss_stale_reviews]=true \
    -F required_pull_request_reviews[require_code_owner_reviews]=false \
    -F required_pull_request_reviews[required_approving_review_count]="${approvals}" \
    -F required_pull_request_reviews[require_last_push_approval]=true \
    -f restrictions:=null \
    -F required_linear_history=true \
    -F allow_force_pushes=false \
    -F allow_deletions=false \
    -F block_creations=false \
    -F required_conversation_resolution=true \
    -F lock_branch=false \
    -F allow_fork_syncing=false 2>&1
)"
status=$?
set -e

if [[ ${status} -ne 0 ]]; then
  if printf '%s' "${output}" | grep -q "Upgrade to GitHub Pro or make this repository public"; then
    cat >&2 <<EOF
GitHub rejected branch protection for ${repo}@${branch}.

This repository is private, and GitHub only enables protected branches on private repositories
for GitHub Pro, GitHub Team, GitHub Enterprise Cloud, or GitHub Enterprise Server.

Options:
  - upgrade the owner account to GitHub Pro
  - move the repository to an organization on GitHub Team/Enterprise
  - make the repository public

The GitHub App branch + PR workflow can still be used without this, but direct pushes to ${branch}
will not be blocked server-side until one of the options above is in place.
EOF
    exit 1
  fi

  printf '%s\n' "${output}" >&2
  exit "${status}"
fi

echo "Protected ${repo}@${branch}."
echo "Direct pushes to ${branch} should now be blocked by GitHub server-side rules."
