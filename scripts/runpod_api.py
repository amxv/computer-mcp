#!/usr/bin/env python3
import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from shutil import which
from typing import Any, Dict, Optional


API_BASE_URL = os.environ.get("RUNPOD_API_BASE_URL", "https://rest.runpod.io/v1")
REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_PORTS = ["8080/http", "22/tcp"]
DEFAULT_CATEGORY = "CPU"
DEFAULT_COMPUTE_TYPE = "CPU"
DEFAULT_CLOUD_TYPE = "SECURE"
DEFAULT_CPU_FLAVOR = "cpu3c"
DEFAULT_CPU_FLAVOR_PRIORITY = "custom"
DEFAULT_VCPU_COUNT = 2
DEFAULT_CONTAINER_DISK_GB = 5
DEFAULT_VOLUME_GB = 0
DEFAULT_VOLUME_MOUNT_PATH = "/workspace"
DEFAULT_TEMPLATE_README = "computer-mcp Runpod CPU template using the dedicated runpod image"
DEFAULT_HTTP_BIND_PORT = 8080
DEFAULT_READER_APP_ID = "3124699"
DEFAULT_READER_INSTALLATION_ID = "117338153"
DEFAULT_PUBLISHER_APP_ID = "3124702"
DEFAULT_PUBLISHER_TARGET_ID = "amxv/computer-mcp"
DEFAULT_PUBLISHER_TARGET_REPO = "amxv/computer-mcp"
DEFAULT_PUBLISHER_INSTALLATION_ID = "117338603"
DEFAULT_PUBLISHER_DEFAULT_BASE = "main"
DEFAULT_WAIT_TIMEOUT_SECONDS = 300
DEFAULT_WAIT_INTERVAL_SECONDS = 5


def die(message: str) -> "NoReturn":
    print(f"[runpod-api] ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def repo_version() -> str:
    cargo_toml = (REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version = "([^"]+)"$', cargo_toml, flags=re.MULTILINE)
    if not match:
        die("could not determine repository version from Cargo.toml")
    return match.group(1)


def default_runpod_image() -> str:
    return os.environ.get("RUNPOD_IMAGE", f"ghcr.io/amxv/computer-mcp-runpod:v{repo_version()}")


def default_template_name() -> str:
    version_slug = repo_version().replace(".", "-")
    return os.environ.get(
        "RUNPOD_TEMPLATE_NAME",
        f"computer-mcp-runpod-cpu3c-2-4-v{version_slug}",
    )


def default_pod_name() -> str:
    version_slug = repo_version().replace(".", "-")
    return os.environ.get(
        "RUNPOD_POD_NAME",
        f"computer-mcp-runpod-v{version_slug}-cpu3c-2-4",
    )


def keychain_secret(service_name: str) -> Optional[str]:
    if which("security") is None:
        return None
    username = os.environ.get("USER", "")
    result = subprocess.run(
        ["security", "find-generic-password", "-a", username, "-s", service_name, "-w"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    value = result.stdout.strip()
    return value or None


def require_binary(binary: str) -> None:
    if which(binary) is None:
        die(f"required binary not found in PATH: {binary}")


def env_text(name: str) -> Optional[str]:
    value = os.environ.get(name)
    if value is None:
        return None
    value = value.strip()
    return value or None


def env_int(name: str, current: Optional[Any], default: int) -> int:
    value = env_text(name)
    if value is not None:
        return int(value)
    if current is not None:
        return int(current)
    return default


def env_bool(name: str, current: Optional[Any], default: bool) -> bool:
    value = env_text(name)
    if value is not None:
        return value.lower() == "true"
    if current is not None:
        return bool(current)
    return default


def env_value(name: str, current: Optional[Any], default: str) -> str:
    value = env_text(name)
    if value is not None:
        return value
    if current is not None:
        return str(current)
    return default


def read_text(path: Path) -> str:
    if not path.is_file():
        die(f"file not found: {path}")
    return path.read_text(encoding="utf-8").strip()


def latest_download(pattern: str) -> Optional[Path]:
    downloads_dir = Path.home() / "Downloads"
    candidates = [path for path in downloads_dir.glob(pattern) if path.is_file()]
    if not candidates:
        return None
    candidates.sort(key=lambda path: path.stat().st_mtime, reverse=True)
    return candidates[0]


def resolve_runpod_api_key() -> str:
    value = env_text("RUNPOD_API_KEY")
    if value:
        return value
    value = keychain_secret("RUNPOD_API_KEY")
    if value:
        return value
    die("set RUNPOD_API_KEY or add a RUNPOD_API_KEY item to the macOS keychain")


def resolve_computer_mcp_api_key() -> str:
    value = env_text("COMPUTER_MCP_API_KEY")
    if value:
        return value
    value = keychain_secret("COMPUTER_MCP_API_KEY")
    if value:
        return value
    die(
        "set COMPUTER_MCP_API_KEY, COMPUTER_MCP_CONFIG_TOML, or "
        "COMPUTER_MCP_CONFIG_TOML_FILE"
    )


def resolve_ssh_public_key() -> str:
    for env_name in ("SSH_PUBLIC_KEY", "PUBLIC_KEY"):
        value = env_text(env_name)
        if value:
            return value

    for env_name in ("SSH_PUBLIC_KEY_FILE", "PUBLIC_KEY_FILE"):
        value = env_text(env_name)
        if value:
            return read_text(Path(value).expanduser())

    default_path = Path.home() / ".ssh" / "id_ed25519.pub"
    if default_path.is_file():
        return read_text(default_path)

    die("set SSH_PUBLIC_KEY, PUBLIC_KEY, SSH_PUBLIC_KEY_FILE, or PUBLIC_KEY_FILE")


def explicit_ssh_public_key() -> Optional[str]:
    for env_name in ("SSH_PUBLIC_KEY", "PUBLIC_KEY"):
        value = env_text(env_name)
        if value:
            return value

    for env_name in ("SSH_PUBLIC_KEY_FILE", "PUBLIC_KEY_FILE"):
        value = env_text(env_name)
        if value:
            return read_text(Path(value).expanduser())

    return None


def resolve_private_key(env_name: str, file_env_name: str, default_glob: str) -> str:
    value = env_text(env_name)
    if value:
        return value

    file_value = env_text(file_env_name)
    if file_value:
        return read_text(Path(file_value).expanduser())

    latest_file = latest_download(default_glob)
    if latest_file is not None:
        return read_text(latest_file)

    die(
        f"set {env_name}, set {file_env_name}, or place a matching key file in "
        f"~/Downloads ({default_glob})"
    )


def explicit_private_key(env_name: str, file_env_name: str) -> Optional[str]:
    value = env_text(env_name)
    if value:
        return value

    file_value = env_text(file_env_name)
    if file_value:
        return read_text(Path(file_value).expanduser())

    return None


def resolve_config_toml() -> str:
    inline_value = env_text("COMPUTER_MCP_CONFIG_TOML")
    if inline_value:
        return inline_value

    file_value = env_text("COMPUTER_MCP_CONFIG_TOML_FILE")
    if file_value:
        return read_text(Path(file_value).expanduser())

    api_key = resolve_computer_mcp_api_key()
    reader_app_id = os.environ.get("COMPUTER_MCP_READER_APP_ID", DEFAULT_READER_APP_ID)
    reader_installation_id = os.environ.get(
        "COMPUTER_MCP_READER_INSTALLATION_ID",
        DEFAULT_READER_INSTALLATION_ID,
    )
    publisher_app_id = os.environ.get(
        "COMPUTER_MCP_PUBLISHER_APP_ID",
        DEFAULT_PUBLISHER_APP_ID,
    )
    publisher_target_id = os.environ.get(
        "COMPUTER_MCP_PUBLISHER_TARGET_ID",
        DEFAULT_PUBLISHER_TARGET_ID,
    )
    publisher_target_repo = os.environ.get(
        "COMPUTER_MCP_PUBLISHER_TARGET_REPO",
        DEFAULT_PUBLISHER_TARGET_REPO,
    )
    publisher_installation_id = os.environ.get(
        "COMPUTER_MCP_PUBLISHER_INSTALLATION_ID",
        DEFAULT_PUBLISHER_INSTALLATION_ID,
    )
    publisher_default_base = os.environ.get(
        "COMPUTER_MCP_PUBLISHER_DEFAULT_BASE",
        DEFAULT_PUBLISHER_DEFAULT_BASE,
    )
    http_bind_port = os.environ.get(
        "COMPUTER_MCP_HTTP_BIND_PORT",
        str(DEFAULT_HTTP_BIND_PORT),
    )

    return "\n".join(
        [
            f'api_key = "{api_key}"',
            f"http_bind_port = {http_bind_port}",
            f"reader_app_id = {reader_app_id}",
            f"reader_installation_id = {reader_installation_id}",
            f"publisher_app_id = {publisher_app_id}",
            "",
            "[[publisher_targets]]",
            f'id = "{publisher_target_id}"',
            f'repo = "{publisher_target_repo}"',
            f'default_base = "{publisher_default_base}"',
            f"installation_id = {publisher_installation_id}",
        ]
    )


def explicit_config_toml() -> Optional[str]:
    inline_value = env_text("COMPUTER_MCP_CONFIG_TOML")
    if inline_value:
        return inline_value

    file_value = env_text("COMPUTER_MCP_CONFIG_TOML_FILE")
    if file_value:
        return read_text(Path(file_value).expanduser())

    return None


def resolve_ports(current_ports: Optional[list[Any]] = None) -> list[str]:
    raw = os.environ.get("RUNPOD_PORTS")
    if raw is not None and raw.strip() != "":
        return [part.strip() for part in raw.split(",") if part.strip()]
    if current_ports is not None:
        return [str(part) for part in current_ports]
    return list(DEFAULT_PORTS)


def common_env_payload(base_env: Optional[Dict[str, Any]] = None) -> Dict[str, str]:
    payload: Dict[str, str] = {
        str(key): str(value) for key, value in (base_env or {}).items() if value is not None
    }

    auto_start = env_text("COMPUTER_MCP_AUTO_START")
    if auto_start is not None:
        payload["COMPUTER_MCP_AUTO_START"] = auto_start
    else:
        payload.setdefault("COMPUTER_MCP_AUTO_START", "1")

    force_reconfigure = env_text("COMPUTER_MCP_FORCE_RECONFIGURE")
    if force_reconfigure is not None:
        payload["COMPUTER_MCP_FORCE_RECONFIGURE"] = force_reconfigure
    else:
        payload.setdefault("COMPUTER_MCP_FORCE_RECONFIGURE", "1")

    config_toml = explicit_config_toml()
    if config_toml is not None:
        payload["COMPUTER_MCP_CONFIG_TOML"] = config_toml
    elif "COMPUTER_MCP_CONFIG_TOML" not in payload:
        payload["COMPUTER_MCP_CONFIG_TOML"] = resolve_config_toml()

    reader_key = explicit_private_key(
        "COMPUTER_MCP_READER_PRIVATE_KEY",
        "COMPUTER_MCP_READER_PRIVATE_KEY_FILE",
    )
    if reader_key is not None:
        payload["COMPUTER_MCP_READER_PRIVATE_KEY"] = reader_key
    elif "COMPUTER_MCP_READER_PRIVATE_KEY" not in payload:
        payload["COMPUTER_MCP_READER_PRIVATE_KEY"] = resolve_private_key(
            "COMPUTER_MCP_READER_PRIVATE_KEY",
            "COMPUTER_MCP_READER_PRIVATE_KEY_FILE",
            "amxv-computer-mcp-reader*.private-key.pem",
        )

    publisher_key = explicit_private_key(
        "COMPUTER_MCP_PUBLISHER_PRIVATE_KEY",
        "COMPUTER_MCP_PUBLISHER_PRIVATE_KEY_FILE",
    )
    if publisher_key is not None:
        payload["COMPUTER_MCP_PUBLISHER_PRIVATE_KEY"] = publisher_key
    elif "COMPUTER_MCP_PUBLISHER_PRIVATE_KEY" not in payload:
        payload["COMPUTER_MCP_PUBLISHER_PRIVATE_KEY"] = resolve_private_key(
            "COMPUTER_MCP_PUBLISHER_PRIVATE_KEY",
            "COMPUTER_MCP_PUBLISHER_PRIVATE_KEY_FILE",
            "amxv-computer-mcp-publisher*.private-key.pem",
        )

    public_key = explicit_ssh_public_key()
    if public_key is not None:
        payload["PUBLIC_KEY"] = public_key
        payload["SSH_PUBLIC_KEY"] = public_key
    elif "PUBLIC_KEY" not in payload and "SSH_PUBLIC_KEY" not in payload:
        default_public_key = resolve_ssh_public_key()
        payload["PUBLIC_KEY"] = default_public_key
        payload["SSH_PUBLIC_KEY"] = default_public_key
    elif "PUBLIC_KEY" in payload and "SSH_PUBLIC_KEY" not in payload:
        payload["SSH_PUBLIC_KEY"] = payload["PUBLIC_KEY"]
    elif "SSH_PUBLIC_KEY" in payload and "PUBLIC_KEY" not in payload:
        payload["PUBLIC_KEY"] = payload["SSH_PUBLIC_KEY"]

    public_host = env_text("COMPUTER_MCP_PUBLIC_HOST")
    if public_host is not None:
        payload["COMPUTER_MCP_PUBLIC_HOST"] = public_host

    return payload


def template_payload(
    base_env: Optional[Dict[str, Any]] = None,
    current_template: Optional[Dict[str, Any]] = None,
    prefer_repo_image: bool = False,
) -> Dict[str, Any]:
    return {
        "category": env_value(
            "RUNPOD_TEMPLATE_CATEGORY",
            current_template.get("category") if current_template else None,
            DEFAULT_CATEGORY,
        ),
        "containerDiskInGb": env_int(
            "RUNPOD_CONTAINER_DISK_GB",
            current_template.get("containerDiskInGb") if current_template else None,
            DEFAULT_CONTAINER_DISK_GB,
        ),
        "dockerEntrypoint": list((current_template or {}).get("dockerEntrypoint") or []),
        "dockerStartCmd": list((current_template or {}).get("dockerStartCmd") or []),
        "env": common_env_payload(base_env),
        "imageName": env_value(
            "RUNPOD_IMAGE",
            None if prefer_repo_image else current_template.get("imageName") if current_template else None,
            default_runpod_image(),
        ),
        "isPublic": env_bool(
            "RUNPOD_TEMPLATE_IS_PUBLIC",
            current_template.get("isPublic") if current_template else None,
            False,
        ),
        "isServerless": False,
        "name": env_value(
            "RUNPOD_TEMPLATE_NAME",
            current_template.get("name") if current_template else None,
            default_template_name(),
        ),
        "ports": resolve_ports((current_template or {}).get("ports")),
        "readme": env_value(
            "RUNPOD_TEMPLATE_README",
            current_template.get("readme") if current_template else None,
            DEFAULT_TEMPLATE_README,
        ),
        "volumeInGb": env_int(
            "RUNPOD_VOLUME_GB",
            current_template.get("volumeInGb") if current_template else None,
            DEFAULT_VOLUME_GB,
        ),
        "volumeMountPath": env_value(
            "RUNPOD_VOLUME_MOUNT_PATH",
            current_template.get("volumeMountPath") if current_template else None,
            DEFAULT_VOLUME_MOUNT_PATH,
        ),
    }


def template_update_payload(
    base_env: Optional[Dict[str, Any]] = None,
    current_template: Optional[Dict[str, Any]] = None,
    prefer_repo_image: bool = False,
) -> Dict[str, Any]:
    payload = template_payload(
        base_env=base_env,
        current_template=current_template,
        prefer_repo_image=prefer_repo_image,
    )
    payload.pop("category", None)
    payload.pop("isServerless", None)
    return payload


def pod_payload(
    base_env: Optional[Dict[str, Any]] = None,
    current_pod: Optional[Dict[str, Any]] = None,
    prefer_repo_image: bool = False,
) -> Dict[str, Any]:
    return {
        "cloudType": env_value(
            "RUNPOD_CLOUD_TYPE",
            current_pod.get("cloudType") if current_pod else None,
            DEFAULT_CLOUD_TYPE,
        ),
        "computeType": env_value(
            "RUNPOD_COMPUTE_TYPE",
            current_pod.get("computeType") if current_pod else None,
            DEFAULT_COMPUTE_TYPE,
        ),
        "containerDiskInGb": env_int(
            "RUNPOD_CONTAINER_DISK_GB",
            current_pod.get("containerDiskInGb") if current_pod else None,
            DEFAULT_CONTAINER_DISK_GB,
        ),
        "cpuFlavorIds": [
            env_value(
                "RUNPOD_CPU_FLAVOR",
                current_pod.get("cpuFlavorId") if current_pod else None,
                DEFAULT_CPU_FLAVOR,
            )
        ],
        "cpuFlavorPriority": env_value(
            "RUNPOD_CPU_FLAVOR_PRIORITY",
            current_pod.get("cpuFlavorPriority") if current_pod else None,
            DEFAULT_CPU_FLAVOR_PRIORITY,
        ),
        "dockerEntrypoint": list((current_pod or {}).get("dockerEntrypoint") or []),
        "dockerStartCmd": list((current_pod or {}).get("dockerStartCmd") or []),
        "env": common_env_payload(base_env),
        "imageName": env_value(
            "RUNPOD_IMAGE",
            None if prefer_repo_image else current_pod.get("imageName") if current_pod else None,
            default_runpod_image(),
        ),
        "name": env_value(
            "RUNPOD_POD_NAME",
            current_pod.get("name") if current_pod else None,
            default_pod_name(),
        ),
        "ports": resolve_ports((current_pod or {}).get("ports")),
        "supportPublicIp": True,
        "vcpuCount": env_int(
            "RUNPOD_VCPU_COUNT",
            current_pod.get("vcpuCount") if current_pod else None,
            DEFAULT_VCPU_COUNT,
        ),
        "volumeInGb": env_int(
            "RUNPOD_VOLUME_GB",
            current_pod.get("volumeInGb") if current_pod else None,
            DEFAULT_VOLUME_GB,
        ),
        "volumeMountPath": env_value(
            "RUNPOD_VOLUME_MOUNT_PATH",
            current_pod.get("volumeMountPath") if current_pod else None,
            DEFAULT_VOLUME_MOUNT_PATH,
        ),
    }


def pod_update_payload(
    base_env: Optional[Dict[str, Any]] = None,
    current_pod: Optional[Dict[str, Any]] = None,
    prefer_repo_image: bool = False,
) -> Dict[str, Any]:
    payload = pod_payload(
        base_env=base_env,
        current_pod=current_pod,
        prefer_repo_image=prefer_repo_image,
    )
    for key in (
        "cloudType",
        "computeType",
        "cpuFlavorIds",
        "cpuFlavorPriority",
        "supportPublicIp",
        "vcpuCount",
    ):
        payload.pop(key, None)
    return payload


def api_request(method: str, path: str, payload: Optional[Dict[str, Any]] = None) -> Any:
    url = f"{API_BASE_URL}{path}"
    data = None
    headers = {
        "Authorization": f"Bearer {resolve_runpod_api_key()}",
        "Accept": "application/json",
    }
    if payload is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request) as response:
            raw = response.read().decode("utf-8")
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        die(f"{method} {url} failed with HTTP {exc.code}: {body}")
    except urllib.error.URLError as exc:
        die(f"{method} {url} failed: {exc}")


def fetch_template(template_id: str) -> Dict[str, Any]:
    return api_request("GET", f"/templates/{template_id}")


def fetch_pod(pod_id: str) -> Dict[str, Any]:
    return api_request("GET", f"/pods/{pod_id}")


def resolve_source_env(
    from_template_id: Optional[str] = None,
    from_pod_id: Optional[str] = None,
) -> Optional[Dict[str, Any]]:
    if from_template_id and from_pod_id:
        die("pass only one of --from-template-id or --from-pod-id")
    if from_template_id:
        return dict(fetch_template(from_template_id).get("env") or {})
    if from_pod_id:
        return dict(fetch_pod(from_pod_id).get("env") or {})
    return None


def redacted_env(env_payload: Dict[str, Any]) -> Dict[str, Any]:
    redacted = dict(env_payload)
    for key in list(redacted):
        if "KEY" in key or "PRIVATE" in key or key == "COMPUTER_MCP_CONFIG_TOML":
            redacted[key] = "<redacted>"
    return redacted


def template_summary(template: Dict[str, Any]) -> Dict[str, Any]:
    summary = {
        "id": template.get("id"),
        "name": template.get("name"),
        "imageName": template.get("imageName"),
        "category": template.get("category"),
        "ports": template.get("ports"),
        "containerDiskInGb": template.get("containerDiskInGb"),
        "volumeInGb": template.get("volumeInGb"),
        "volumeMountPath": template.get("volumeMountPath"),
        "readme": template.get("readme"),
    }
    if "env" in template:
        summary["env"] = redacted_env(template["env"])
    return summary


def pod_summary(pod: Dict[str, Any]) -> Dict[str, Any]:
    summary = {
        "id": pod.get("id"),
        "name": pod.get("name"),
        "desiredStatus": pod.get("desiredStatus"),
        "lastStartedAt": pod.get("lastStartedAt"),
        "lastStatusChange": pod.get("lastStatusChange"),
        "publicIp": pod.get("publicIp"),
        "portMappings": pod.get("portMappings"),
        "ports": pod.get("ports"),
        "imageName": pod.get("imageName"),
        "cpuFlavorId": pod.get("cpuFlavorId"),
        "vcpuCount": pod.get("vcpuCount"),
        "memoryInGb": pod.get("memoryInGb"),
        "containerDiskInGb": pod.get("containerDiskInGb"),
        "volumeInGb": pod.get("volumeInGb"),
        "volumeMountPath": pod.get("volumeMountPath"),
    }
    if "env" in pod:
        summary["env"] = redacted_env(pod["env"])
    return summary


def print_json(data: Dict[str, Any]) -> None:
    print(json.dumps(data, indent=2, sort_keys=True))


def show_request(method: str, path: str, payload: Optional[Dict[str, Any]]) -> None:
    output = {"method": method, "url": f"{API_BASE_URL}{path}"}
    if payload is not None:
        output_payload = dict(payload)
        if "env" in output_payload and isinstance(output_payload["env"], dict):
            output_payload["env"] = redacted_env(output_payload["env"])
        output["payload"] = output_payload
    print_json(output)


def wait_ready(pod_id: str, timeout_seconds: int) -> Dict[str, Any]:
    deadline = time.time() + timeout_seconds
    while time.time() < deadline:
        pod = api_request("GET", f"/pods/{pod_id}")
        public_ip = pod.get("publicIp") or ""
        port_mappings = pod.get("portMappings") or {}
        ssh_port = port_mappings.get("22")
        if public_ip and ssh_port:
            return pod
        time.sleep(DEFAULT_WAIT_INTERVAL_SECONDS)
    die(
        f"pod {pod_id} did not expose a public IP and SSH port within "
        f"{timeout_seconds} seconds"
    )


def run_ssh_verification(pod: Dict[str, Any]) -> None:
    public_ip = pod.get("publicIp")
    port_mappings = pod.get("portMappings") or {}
    ssh_port = port_mappings.get("22")
    if not public_ip or not ssh_port:
        die("pod is missing publicIp or portMappings[22]")

    ssh_key_path = Path(os.environ.get("SSH_PRIVATE_KEY_FILE", "~/.ssh/id_ed25519")).expanduser()
    if not ssh_key_path.is_file():
        die(f"SSH private key not found: {ssh_key_path}")

    command = (
        "computer-mcp --version && "
        "echo '---status---' && computer-mcp status && "
        "echo '---health-http---' && curl -fsS http://127.0.0.1:8080/health && echo"
    )
    result = subprocess.run(
        [
            "ssh",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-p",
            str(ssh_port),
            "-i",
            str(ssh_key_path),
            f"root@{public_ip}",
            command,
        ],
        text=True,
    )
    if result.returncode != 0:
        die(f"SSH verification failed for pod {pod.get('id')}")


def run_proxy_health_check(pod_id: str) -> None:
    url = f"https://{pod_id}-8080.proxy.runpod.net/health"
    try:
        with urllib.request.urlopen(url) as response:
            body = response.read().decode("utf-8", errors="replace").strip()
            print(body)
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        die(f"proxy health check failed with HTTP {exc.code}: {body}")
    except urllib.error.URLError as exc:
        die(f"proxy health check failed: {exc}")


def proxy_health_body(pod_id: str) -> str:
    url = f"https://{pod_id}-8080.proxy.runpod.net/health"
    try:
        with urllib.request.urlopen(url) as response:
            return response.read().decode("utf-8", errors="replace").strip()
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        die(f"proxy health check failed with HTTP {exc.code}: {body}")
    except urllib.error.URLError as exc:
        die(f"proxy health check failed: {exc}")


def handle_template_create(args: argparse.Namespace) -> None:
    payload = template_payload(
        base_env=resolve_source_env(
            from_template_id=args.from_template_id,
            from_pod_id=args.from_pod_id,
        )
    )
    if args.dry_run:
        show_request("POST", "/templates", payload)
        return
    print_json(template_summary(api_request("POST", "/templates", payload)))


def handle_template_update(args: argparse.Namespace) -> None:
    current_template = fetch_template(args.template_id)
    source_env = resolve_source_env(
        from_template_id=args.from_template_id,
        from_pod_id=args.from_pod_id,
    )
    if source_env is None:
        source_env = current_template.get("env") or {}
    payload = template_update_payload(
        base_env=source_env,
        current_template=current_template,
    )
    path = f"/templates/{args.template_id}/update"
    if args.dry_run:
        show_request("POST", path, payload)
        return
    print_json(template_summary(api_request("POST", path, payload)))


def handle_template_get(args: argparse.Namespace) -> None:
    print_json(template_summary(fetch_template(args.template_id)))


def handle_pod_create(args: argparse.Namespace) -> None:
    payload = pod_payload(
        base_env=resolve_source_env(
            from_template_id=args.from_template_id,
            from_pod_id=args.from_pod_id,
        )
    )
    if args.dry_run:
        show_request("POST", "/pods", payload)
        return
    print_json(pod_summary(api_request("POST", "/pods", payload)))


def handle_pod_get(args: argparse.Namespace) -> None:
    print_json(pod_summary(fetch_pod(args.pod_id)))


def handle_pod_update(args: argparse.Namespace) -> None:
    current_pod = fetch_pod(args.pod_id)
    source_env = resolve_source_env(
        from_template_id=args.from_template_id,
        from_pod_id=args.from_pod_id,
    )
    if source_env is None:
        source_env = current_pod.get("env") or {}
    payload = pod_update_payload(
        base_env=source_env,
        current_pod=current_pod,
    )
    path = f"/pods/{args.pod_id}/update"
    if args.dry_run:
        show_request("POST", path, payload)
        return
    print_json(pod_summary(api_request("POST", path, payload)))


def handle_pod_action(args: argparse.Namespace) -> None:
    path = f"/pods/{args.pod_id}/{args.action}"
    if args.dry_run:
        show_request("POST", path, None)
        return
    print_json(pod_summary(api_request("POST", path)))


def handle_pod_wait_ready(args: argparse.Namespace) -> None:
    print_json(pod_summary(wait_ready(args.pod_id, args.timeout_seconds)))


def handle_pod_verify(args: argparse.Namespace) -> None:
    require_binary("ssh")
    pod = wait_ready(args.pod_id, args.timeout_seconds)
    run_ssh_verification(pod)
    run_proxy_health_check(args.pod_id)


def handle_rollout_image(args: argparse.Namespace) -> None:
    current_template = fetch_template(args.template_id)
    current_pod = fetch_pod(args.pod_id)
    source_env = (
        dict(current_pod.get("env") or {})
        if args.env_source == "pod"
        else dict(current_template.get("env") or {})
    )

    template_path = f"/templates/{args.template_id}/update"
    pod_path = f"/pods/{args.pod_id}/update"
    template_update = template_update_payload(
        base_env=source_env,
        current_template=current_template,
        prefer_repo_image=True,
    )
    pod_update = pod_update_payload(
        base_env=source_env,
        current_pod=current_pod,
        prefer_repo_image=True,
    )

    if args.dry_run:
        print_json(
            {
                "templateUpdate": {
                    "method": "POST",
                    "url": f"{API_BASE_URL}{template_path}",
                    "payload": {
                        **template_update,
                        "env": redacted_env(template_update["env"]),
                    },
                },
                "podUpdate": {
                    "method": "POST",
                    "url": f"{API_BASE_URL}{pod_path}",
                    "payload": {
                        **pod_update,
                        "env": redacted_env(pod_update["env"]),
                    },
                },
                "resetPod": args.reset,
                "waitReady": args.wait_ready or args.verify,
                "verify": args.verify,
            }
        )
        return

    result: Dict[str, Any] = {
        "template": template_summary(api_request("POST", template_path, template_update)),
        "pod": pod_summary(api_request("POST", pod_path, pod_update)),
    }
    if args.reset:
        result["reset"] = pod_summary(api_request("POST", f"/pods/{args.pod_id}/reset"))

    ready_pod: Optional[Dict[str, Any]] = None
    if args.wait_ready or args.verify:
        ready_pod = wait_ready(args.pod_id, args.timeout_seconds)
        result["ready"] = pod_summary(ready_pod)

    if args.verify:
        require_binary("ssh")
        if ready_pod is None:
            ready_pod = wait_ready(args.pod_id, args.timeout_seconds)
        run_ssh_verification(ready_pod)
        result["verification"] = {
            "ssh": "ok",
            "proxyHealth": proxy_health_body(args.pod_id),
        }

    print_json(result)


def add_source_env_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--from-template-id",
        help="Reuse env/config from an existing template instead of reconstructing it locally",
    )
    parser.add_argument(
        "--from-pod-id",
        help="Reuse env/config from an existing pod instead of reconstructing it locally",
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Manage Runpod templates and pods for computer-mcp using the official Runpod REST API.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    template_parser = subparsers.add_parser("template", help="Manage Runpod templates")
    template_subparsers = template_parser.add_subparsers(dest="template_command", required=True)

    template_create = template_subparsers.add_parser("create", help="Create a template")
    add_source_env_args(template_create)
    template_create.add_argument("--dry-run", action="store_true", help="Print request payload instead of sending it")
    template_create.set_defaults(func=handle_template_create)

    template_update = template_subparsers.add_parser("update", help="Update an existing template")
    template_update.add_argument("template_id", help="Runpod template id")
    add_source_env_args(template_update)
    template_update.add_argument("--dry-run", action="store_true", help="Print request payload instead of sending it")
    template_update.set_defaults(func=handle_template_update)

    template_get = template_subparsers.add_parser("get", help="Fetch a template")
    template_get.add_argument("template_id", help="Runpod template id")
    template_get.set_defaults(func=handle_template_get)

    pod_parser = subparsers.add_parser("pod", help="Manage Runpod pods")
    pod_subparsers = pod_parser.add_subparsers(dest="pod_command", required=True)

    pod_create = pod_subparsers.add_parser("create", help="Create a pod")
    add_source_env_args(pod_create)
    pod_create.add_argument("--dry-run", action="store_true", help="Print request payload instead of sending it")
    pod_create.set_defaults(func=handle_pod_create)

    pod_get = pod_subparsers.add_parser("get", help="Fetch a pod")
    pod_get.add_argument("pod_id", help="Runpod pod id")
    pod_get.set_defaults(func=handle_pod_get)

    pod_update = pod_subparsers.add_parser("update", help="Update an existing pod")
    pod_update.add_argument("pod_id", help="Runpod pod id")
    add_source_env_args(pod_update)
    pod_update.add_argument("--dry-run", action="store_true", help="Print request payload instead of sending it")
    pod_update.set_defaults(func=handle_pod_update)

    for action in ("start", "stop", "restart", "reset"):
        action_parser = pod_subparsers.add_parser(action, help=f"{action.capitalize()} a pod")
        action_parser.add_argument("pod_id", help="Runpod pod id")
        action_parser.add_argument("--dry-run", action="store_true", help="Print request path instead of sending it")
        action_parser.set_defaults(func=handle_pod_action, action=action)

    pod_wait = pod_subparsers.add_parser("wait-ready", help="Wait until a pod exposes public IP and SSH")
    pod_wait.add_argument("pod_id", help="Runpod pod id")
    pod_wait.add_argument(
        "--timeout-seconds",
        type=int,
        default=int(os.environ.get("RUNPOD_WAIT_TIMEOUT_SECONDS", str(DEFAULT_WAIT_TIMEOUT_SECONDS))),
        help="Maximum time to wait before failing",
    )
    pod_wait.set_defaults(func=handle_pod_wait_ready)

    pod_verify = pod_subparsers.add_parser(
        "verify",
        help="Wait for a pod, verify SSH, and hit the public /health endpoint",
    )
    pod_verify.add_argument("pod_id", help="Runpod pod id")
    pod_verify.add_argument(
        "--timeout-seconds",
        type=int,
        default=int(os.environ.get("RUNPOD_WAIT_TIMEOUT_SECONDS", str(DEFAULT_WAIT_TIMEOUT_SECONDS))),
        help="Maximum time to wait before failing",
    )
    pod_verify.set_defaults(func=handle_pod_verify)

    rollout = subparsers.add_parser(
        "rollout-image",
        help="Update an existing template and pod to the current image while reusing existing env/config",
    )
    rollout.add_argument("template_id", help="Runpod template id")
    rollout.add_argument("pod_id", help="Runpod pod id")
    rollout.add_argument(
        "--env-source",
        choices=("pod", "template"),
        default="pod",
        help="Use env/config from the live pod or current template as the rollout source",
    )
    rollout.add_argument(
        "--no-reset",
        dest="reset",
        action="store_false",
        help="Update metadata only and skip the pod reset",
    )
    rollout.add_argument(
        "--wait-ready",
        action="store_true",
        help="Wait for the pod to expose SSH again after the rollout",
    )
    rollout.add_argument(
        "--verify",
        action="store_true",
        help="Run SSH verification and hit the public /health endpoint after the rollout",
    )
    rollout.add_argument(
        "--timeout-seconds",
        type=int,
        default=int(os.environ.get("RUNPOD_WAIT_TIMEOUT_SECONDS", str(DEFAULT_WAIT_TIMEOUT_SECONDS))),
        help="Maximum time to wait before failing",
    )
    rollout.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the update/reset plan instead of sending it",
    )
    rollout.set_defaults(func=handle_rollout_image, reset=True)

    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
