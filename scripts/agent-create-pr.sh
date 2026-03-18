#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/agent-create-pr.sh \
    --repo OWNER/REPO \
    --branch agent/task-name \
    --title "Agent: task title" \
    --body "Automated change from computer-mcp agent." \
    --commit-message "agent: implement task"

Options:
  --repo OWNER/REPO           Required. GitHub repository.
  --branch NAME               Required. Branch to create/push.
  --title TEXT                Required. Pull request title.
  --body TEXT                 Optional. Pull request body.
  --body-file PATH            Optional. Pull request body file.
  --base NAME                 Optional. Base branch. Default: main
  --commit-message TEXT       Required when there are uncommitted changes.
  --name TEXT                 Optional git author name.
  --email TEXT                Optional git author email.

Environment:
  GITHUB_APP_ID
  GITHUB_APP_INSTALLATION_ID
  GITHUB_APP_PRIVATE_KEY_PATH
  GITHUB_APP_PERMISSIONS_JSON Optional. Default from mint script.

Notes:
  - The current directory must be a git checkout of the target repo.
  - This script never pushes to main directly. It pushes HEAD to the supplied branch
    and then opens a pull request.
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
branch=""
base="main"
title=""
body="Automated change from computer-mcp agent."
body_file=""
commit_message=""
author_name=""
author_email=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) repo="${2:?missing value for --repo}"; shift 2 ;;
    --branch) branch="${2:?missing value for --branch}"; shift 2 ;;
    --title) title="${2:?missing value for --title}"; shift 2 ;;
    --body) body="${2:?missing value for --body}"; shift 2 ;;
    --body-file) body_file="${2:?missing value for --body-file}"; shift 2 ;;
    --base) base="${2:?missing value for --base}"; shift 2 ;;
    --commit-message) commit_message="${2:?missing value for --commit-message}"; shift 2 ;;
    --name) author_name="${2:?missing value for --name}"; shift 2 ;;
    --email) author_email="${2:?missing value for --email}"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
done

[[ -n "${repo}" ]] || die "--repo is required"
[[ -n "${branch}" ]] || die "--branch is required"
[[ -n "${title}" ]] || die "--title is required"
if [[ -n "${body_file}" ]]; then
  [[ -f "${body_file}" ]] || die "body file not found: ${body_file}"
  body="$(cat "${body_file}")"
fi

require_cmd git
require_cmd gh

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "current directory is not a git repository"

if [[ -n "${author_name}" ]]; then
  git config user.name "${author_name}"
fi
if [[ -n "${author_email}" ]]; then
  git config user.email "${author_email}"
fi

current_branch="$(git rev-parse --abbrev-ref HEAD)"
if [[ "${current_branch}" != "${branch}" ]]; then
  git checkout -B "${branch}"
fi

if [[ -n "$(git status --porcelain)" ]]; then
  [[ -n "${commit_message}" ]] || die "--commit-message is required when the worktree has changes"
  git add -A
  git diff --cached --quiet && die "no staged changes were found after git add -A"
  git commit -m "${commit_message}"
fi

token="$("$(dirname "$0")/mint-gh-app-installation-token.sh")"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

cat > "${tmp_dir}/git-askpass.sh" <<'EOF'
#!/usr/bin/env bash
case "$1" in
  *Username*) printf '%s\n' "x-access-token" ;;
  *Password*) printf '%s\n' "${GITHUB_APP_TOKEN}" ;;
  *) printf '\n' ;;
esac
EOF
chmod 700 "${tmp_dir}/git-askpass.sh"

GITHUB_APP_TOKEN="${token}" \
GIT_ASKPASS="${tmp_dir}/git-askpass.sh" \
GIT_TERMINAL_PROMPT=0 \
git push "https://github.com/${repo}.git" "HEAD:${branch}"

pr_url="$(
  GH_TOKEN="${token}" gh pr create \
    --repo "${repo}" \
    --base "${base}" \
    --head "${branch}" \
    --title "${title}" \
    --body "${body}"
)"

printf '%s\n' "${pr_url}"
