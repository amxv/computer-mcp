#!/usr/bin/env python3
"""Minimal read-only client for the public Zodex Local observability API."""

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


API_VERSION = 1
PRESENTATION_VERSION = 3
EVENT_VERSION = 2
AGENT_ID = re.compile(r"^[a-z0-9]{4}$")


def default_discovery_path() -> Path:
    state_home = os.environ.get("XDG_STATE_HOME")
    if state_home:
        return Path(state_home) / "zodex/local/runtime/discovery.json"
    return Path.home() / ".local/state/zodex/local/runtime/discovery.json"


def read_discovery(path: Path) -> tuple[dict, str]:
    discovery = json.loads(path.read_text())
    observer = discovery["observability"]
    if observer.get("api_version") != API_VERSION:
        raise RuntimeError(
            f"unsupported observer API version: {observer.get('api_version')!r}; "
            f"expected {API_VERSION}"
        )
    if observer.get("presentation_version") != PRESENTATION_VERSION:
        raise RuntimeError(
            "unsupported presentation version: "
            f"{observer.get('presentation_version')!r}; expected {PRESENTATION_VERSION}"
        )
    runtime_id = discovery.get("runtime_id")
    if not isinstance(runtime_id, str) or not runtime_id:
        raise RuntimeError("discovery document has no runtime_id")

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
            "User-Agent": "zodex-local-observer-example/3",
        },
    )


def get_json(url: str, token: str) -> dict:
    with urllib.request.urlopen(authorized_request(url, token), timeout=10) as response:
        return json.load(response)


def verify_runtime(document: dict, runtime_id: str, label: str) -> None:
    if document.get("runtime_id") != runtime_id:
        raise RuntimeError(
            f"{label} belongs to runtime {document.get('runtime_id')!r}, "
            f"expected {runtime_id!r}; reread discovery"
        )


def print_initial_state(base_url: str, token: str, runtime_id: str, agent: str | None) -> None:
    agents_query = urllib.parse.urlencode({"runtime": "current"})
    agents = get_json(f"{base_url}/v1/agents?{agents_query}", token)
    verify_runtime(agents, runtime_id, "Agent list")

    timeline_params: dict[str, str | int] = {"limit": 20}
    if agent:
        timeline_params["agent_id"] = agent
    timeline = get_json(
        f"{base_url}/v1/timeline?{urllib.parse.urlencode(timeline_params)}", token
    )
    verify_runtime(timeline, runtime_id, "timeline")
    if timeline.get("presentation_version") != PRESENTATION_VERSION:
        raise RuntimeError(
            f"timeline presentation version is {timeline.get('presentation_version')!r}, "
            f"expected {PRESENTATION_VERSION}"
        )

    print(
        json.dumps(
            {
                "runtime_id": runtime_id,
                "agents": agents.get("agents", []),
                "timeline": timeline,
            },
            indent=2,
        )
    )


def stream_events(
    base_url: str,
    token: str,
    runtime_id: str,
    agent: str | None,
    max_events: int,
) -> None:
    query = ""
    if agent:
        # Filter all event types to one Agent. A multi-column UI would normally
        # keep global metadata and use output_agent_ids for visible PTY streams.
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
                    payload = json.loads("\n".join(data_lines))
                    if payload.get("schema_version") != EVENT_VERSION:
                        raise RuntimeError(
                            "unsupported live event version: "
                            f"{payload.get('schema_version')!r}; expected {EVENT_VERSION}"
                        )
                    verify_runtime(payload, runtime_id, "live event")
                    print(
                        json.dumps(
                            {
                                "id": event_id,
                                "event": event_name,
                                "data": payload,
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
    parser.add_argument("--agent", help="four-character Agent ID for timeline/SSE filtering")
    parser.add_argument(
        "--events", action="store_true", help="stream live SSE after printing initial state"
    )
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
        runtime_id = discovery["runtime_id"]
        base_url = observer["base_url"].rstrip("/")
        print_initial_state(base_url, token, runtime_id, args.agent)
        if args.events:
            stream_events(base_url, token, runtime_id, args.agent, args.max_events)
    except (OSError, KeyError, ValueError, urllib.error.URLError, RuntimeError) as error:
        print(f"zodex-local-observer: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
