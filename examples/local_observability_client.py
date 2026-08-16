#!/usr/bin/env python3
"""Minimal read-only client for the Zodex Local observability API."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


AGENT_ID = re.compile(r"^[a-z0-9]{4}$")


def default_discovery_path() -> Path:
    state_home = os.environ.get("XDG_STATE_HOME")
    if state_home:
        return Path(state_home) / "zodex/local/runtime/discovery.json"
    return Path.home() / ".local/state/zodex/local/runtime/discovery.json"


def read_discovery(path: Path) -> tuple[dict, str]:
    discovery = json.loads(path.read_text())
    observer = discovery["observability"]
    token_path = Path(observer["bearer_token_path"])
    token = token_path.read_text().strip()
    if len(token) < 32:
        raise RuntimeError("observer bearer is missing or unexpectedly short")
    return discovery, token


def authorized_request(url: str, token: str) -> urllib.request.Request:
    return urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json, text/event-stream",
            "User-Agent": "zodex-local-observer-example/1",
        },
    )


def get_json(url: str, token: str) -> dict:
    with urllib.request.urlopen(authorized_request(url, token), timeout=10) as response:
        return json.load(response)


def print_agents(base_url: str, token: str) -> None:
    document = get_json(f"{base_url}/v1/agents", token)
    print(json.dumps(document, indent=2))


def stream_events(base_url: str, token: str, agent: str | None, max_events: int) -> None:
    query = ""
    if agent:
        query = "?" + urllib.parse.urlencode({"agent_id": agent})
    request = authorized_request(f"{base_url}/v1/events{query}", token)
    seen = 0
    event_name: str | None = None
    event_id: str | None = None
    data_lines: list[str] = []

    with urllib.request.urlopen(request) as response:
        for raw_line in response:
            line = raw_line.decode("utf-8", errors="replace").rstrip("\r\n")
            if not line:
                if data_lines:
                    payload = "\n".join(data_lines)
                    print(
                        json.dumps(
                            {
                                "id": event_id,
                                "event": event_name,
                                "data": json.loads(payload),
                            },
                            separators=(",", ":"),
                        )
                    )
                    seen += 1
                    if max_events and seen >= max_events:
                        return
                event_name = None
                event_id = None
                data_lines = []
                continue
            if line.startswith(":"):
                continue
            field, separator, value = line.partition(":")
            if separator and value.startswith(" "):
                value = value[1:]
            if field == "event":
                event_name = value
            elif field == "id":
                event_id = value
            elif field == "data":
                data_lines.append(value)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Read the versioned Zodex Local observability API."
    )
    parser.add_argument(
        "--discovery",
        type=Path,
        default=default_discovery_path(),
        help="path to the active Local discovery.json",
    )
    parser.add_argument("--agent", help="four-character Agent ID for SSE filtering")
    parser.add_argument("--events", action="store_true", help="stream live SSE after listing Agents")
    parser.add_argument(
        "--max-events",
        type=int,
        default=0,
        help="stop after N SSE events (0 means keep streaming)",
    )
    args = parser.parse_args()

    if args.agent and not AGENT_ID.fullmatch(args.agent):
        parser.error("--agent must match [a-z0-9]{4}")
    if args.max_events < 0:
        parser.error("--max-events must be non-negative")

    try:
        discovery, token = read_discovery(args.discovery)
        observer = discovery["observability"]
        if observer.get("api_version") != 1:
            raise RuntimeError(f"unsupported observer API version: {observer.get('api_version')!r}")
        base_url = observer["base_url"].rstrip("/")
        print_agents(base_url, token)
        if args.events:
            stream_events(base_url, token, args.agent, args.max_events)
    except (OSError, KeyError, ValueError, urllib.error.URLError, RuntimeError) as error:
        print(f"zodex-local-observer: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
