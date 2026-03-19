#!/usr/bin/env python3
import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple


API_BASE_URL = "https://api.github.com"
API_VERSION = "2026-03-10"
DEFAULT_INTERVAL_SECONDS = 2.0
BAD_CONCLUSIONS = {
    "action_required",
    "cancelled",
    "failure",
    "stale",
    "startup_failure",
    "timed_out",
}
RUN_SUCCESS_CONCLUSIONS = {"success", "neutral", "skipped"}


def die(message: str) -> "NoReturn":
    print(f"[gha-watch] ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def env_text(name: str) -> Optional[str]:
    value = os.environ.get(name)
    if value is None:
        return None
    value = value.strip()
    return value or None


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def resolve_repo_slug() -> str:
    env_value = env_text("GITHUB_REPOSITORY")
    if env_value:
        return env_value

    result = subprocess.run(
        ["git", "remote", "get-url", "origin"],
        cwd=repo_root(),
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        die("set --repo or GITHUB_REPOSITORY, or run inside a git repo with origin configured")

    remote = result.stdout.strip()
    if remote.endswith(".git"):
        remote = remote[:-4]

    ssh_match = re.search(r"github\.com[:/](?P<owner>[^/]+)/(?P<repo>[^/]+)$", remote)
    if ssh_match:
        return f"{ssh_match.group('owner')}/{ssh_match.group('repo')}"

    die("failed to infer owner/repo from git remote origin")


def resolve_github_token() -> str:
    for name in ("GH_TOKEN", "GITHUB_TOKEN"):
        value = env_text(name)
        if value:
            return value

    result = subprocess.run(
        ["gh", "auth", "token"],
        text=True,
        capture_output=True,
    )
    if result.returncode == 0:
        value = result.stdout.strip()
        if value:
            return value

    die("set GH_TOKEN or GITHUB_TOKEN, or authenticate gh CLI with `gh auth login`")


def github_request(token: str, path: str) -> Any:
    request = urllib.request.Request(
        urllib.parse.urljoin(API_BASE_URL, path),
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": API_VERSION,
        },
    )
    try:
        with urllib.request.urlopen(request) as response:
            return json.load(response)
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"GitHub API {exc.code} for {path}: {body}") from exc


def get_run(token: str, repo: str, run_id: int) -> Dict[str, Any]:
    return github_request(token, f"/repos/{repo}/actions/runs/{run_id}")


def get_jobs(token: str, repo: str, run_id: int) -> List[Dict[str, Any]]:
    jobs: List[Dict[str, Any]] = []
    page = 1
    while True:
        payload = github_request(
            token,
            f"/repos/{repo}/actions/runs/{run_id}/jobs?per_page=100&page={page}",
        )
        page_jobs = payload.get("jobs", [])
        jobs.extend(page_jobs)
        if len(page_jobs) < 100:
            return jobs
        page += 1


def collect_failed_steps(job: Dict[str, Any]) -> List[Dict[str, Any]]:
    failures: List[Dict[str, Any]] = []
    for step in job.get("steps", []):
        conclusion = (step.get("conclusion") or "").lower()
        if conclusion in BAD_CONCLUSIONS:
            failures.append(step)
    return failures


def conclusion_is_failure(conclusion: Optional[str]) -> bool:
    if conclusion is None:
        return False
    return conclusion.lower() in BAD_CONCLUSIONS


def format_step(step: Dict[str, Any]) -> str:
    name = step.get("name", "<unknown step>")
    number = step.get("number", "?")
    conclusion = step.get("conclusion", "")
    status = step.get("status", "")
    return f"step {number} `{name}` status={status} conclusion={conclusion}"


def format_job(job: Dict[str, Any]) -> str:
    name = job.get("name", "<unknown job>")
    job_id = job.get("id", "?")
    status = job.get("status", "")
    conclusion = job.get("conclusion", "")
    html_url = job.get("html_url", "")
    return f"job `{name}` id={job_id} status={status} conclusion={conclusion} {html_url}".strip()


def print_job_transitions(
    run_id: int,
    jobs: Iterable[Dict[str, Any]],
    seen_jobs: Dict[Tuple[int, int], Tuple[str, str]],
    seen_steps: Dict[Tuple[int, int, int], Tuple[str, str]],
) -> None:
    for job in jobs:
        job_id = int(job["id"])
        status = job.get("status") or ""
        conclusion = job.get("conclusion") or ""
        job_key = (run_id, job_id)
        job_state = (status, conclusion)
        if seen_jobs.get(job_key) != job_state:
            print(f"[gha-watch] run {run_id}: {format_job(job)}")
            seen_jobs[job_key] = job_state

        for step in job.get("steps", []):
            step_number = int(step["number"])
            step_status = step.get("status") or ""
            step_conclusion = step.get("conclusion") or ""
            step_key = (run_id, job_id, step_number)
            step_state = (step_status, step_conclusion)
            if seen_steps.get(step_key) != step_state:
                print(
                    "[gha-watch] "
                    f"run {run_id}: job `{job.get('name', '<unknown job>')}` {format_step(step)}"
                )
                seen_steps[step_key] = step_state


def check_for_failure(run_id: int, jobs: Iterable[Dict[str, Any]]) -> Optional[str]:
    for job in jobs:
        if conclusion_is_failure(job.get("conclusion")):
            return f"run {run_id} failed: {format_job(job)}"
        failed_steps = collect_failed_steps(job)
        if failed_steps:
            step_text = "; ".join(format_step(step) for step in failed_steps)
            return f"run {run_id} failed: {format_job(job)} :: {step_text}"
    return None


def all_runs_successful(run_states: Dict[int, Dict[str, Any]]) -> bool:
    for run in run_states.values():
        if run.get("status") != "completed":
            return False
        conclusion = (run.get("conclusion") or "").lower()
        if conclusion not in RUN_SUCCESS_CONCLUSIONS:
            return False
    return True


def watch_runs(repo: str, run_ids: List[int], interval_seconds: float) -> int:
    token = resolve_github_token()
    seen_jobs: Dict[Tuple[int, int], Tuple[str, str]] = {}
    seen_steps: Dict[Tuple[int, int, int], Tuple[str, str]] = {}

    while True:
        run_states: Dict[int, Dict[str, Any]] = {}

        for run_id in run_ids:
            run = get_run(token, repo, run_id)
            jobs = get_jobs(token, repo, run_id)
            print_job_transitions(run_id, jobs, seen_jobs, seen_steps)

            failure = check_for_failure(run_id, jobs)
            if failure:
                print(f"[gha-watch] {failure}", file=sys.stderr)
                return 1

            run_states[run_id] = run

        if all_runs_successful(run_states):
            for run_id, run in run_states.items():
                print(
                    "[gha-watch] "
                    f"run {run_id} completed with conclusion={run.get('conclusion')} "
                    f"{run.get('html_url', '')}".strip()
                )
            return 0

        time.sleep(interval_seconds)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Fail-fast watcher for GitHub Actions runs using the official REST API.",
    )
    parser.add_argument(
        "run_ids",
        metavar="RUN_ID",
        nargs="+",
        type=int,
        help="one or more GitHub Actions workflow run IDs to watch",
    )
    parser.add_argument(
        "--repo",
        default=resolve_repo_slug(),
        help="owner/repo slug (defaults to origin remote or GITHUB_REPOSITORY)",
    )
    parser.add_argument(
        "--interval",
        type=float,
        default=DEFAULT_INTERVAL_SECONDS,
        help=f"poll interval in seconds (default: {DEFAULT_INTERVAL_SECONDS})",
    )
    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    if args.interval <= 0:
        die("--interval must be positive")
    raise SystemExit(watch_runs(args.repo, args.run_ids, args.interval))


if __name__ == "__main__":
    main()
