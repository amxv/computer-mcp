#!/usr/bin/env bash
set -euo pipefail

SCRIPT_VERSION="0.1.0"

COMPUTER_MCP_VERSION="${COMPUTER_MCP_VERSION:-latest}"
COMPUTER_MCP_REPO="${COMPUTER_MCP_REPO:-amxv/computer-mcp}"
COMPUTER_MCP_ASSET_URL="${COMPUTER_MCP_ASSET_URL:-}"
COMPUTER_MCP_SOURCE_REF="${COMPUTER_MCP_SOURCE_REF:-main}"
COMPUTER_MCP_BINARY_SOURCE_DIR="${COMPUTER_MCP_BINARY_SOURCE_DIR:-}"
COMPUTER_MCP_INSTALL_DIR="${COMPUTER_MCP_INSTALL_DIR:-/usr/local/bin}"
COMPUTER_MCP_CONFIG_PATH="${COMPUTER_MCP_CONFIG_PATH:-/etc/computer-mcp/config.toml}"
COMPUTER_MCP_STATE_DIR="${COMPUTER_MCP_STATE_DIR:-/var/lib/computer-mcp}"
COMPUTER_MCP_TLS_DIR="${COMPUTER_MCP_TLS_DIR:-${COMPUTER_MCP_STATE_DIR}/tls}"
COMPUTER_MCP_ENABLE_CERTBOT="${COMPUTER_MCP_ENABLE_CERTBOT:-0}"

DISTRO_ID="unknown"
DISTRO_LIKE=""
ARCH="unknown"
TARGET_TRIPLE="unknown"
TMP_DIR=""

log() {
  printf '[computer-mcp install] %s\n' "$*"
}

warn() {
  printf '[computer-mcp install] WARNING: %s\n' "$*" >&2
}

die() {
  printf '[computer-mcp install] ERROR: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]]; then
    rm -rf "${TMP_DIR}"
  fi
}

need_root() {
  if [[ "${EUID}" -ne 0 ]]; then
    die "run as root (for example: curl ... | sudo bash)"
  fi
}

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

detect_platform() {
  [[ "$(uname -s)" == "Linux" ]] || die "Linux only"
  [[ -f /etc/os-release ]] || die "/etc/os-release not found"

  # shellcheck disable=SC1091
  source /etc/os-release
  DISTRO_ID="${ID:-unknown}"
  DISTRO_LIKE="${ID_LIKE:-}"

  case "$(uname -m)" in
    x86_64|amd64)
      ARCH="x86_64"
      TARGET_TRIPLE="x86_64-unknown-linux-gnu"
      ;;
    aarch64|arm64)
      ARCH="aarch64"
      TARGET_TRIPLE="aarch64-unknown-linux-gnu"
      ;;
    *)
      die "unsupported architecture: $(uname -m)"
      ;;
  esac

  if [[ "${DISTRO_ID}" != "ubuntu" && "${DISTRO_ID}" != "debian" ]]; then
    warn "distro ${DISTRO_ID} is not first-class tested for v1; continuing with best effort"
  fi

  log "detected distro=${DISTRO_ID} arch=${ARCH} target=${TARGET_TRIPLE}"
}

install_prerequisites() {
  if command_exists apt-get; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -y
    apt-get install -y --no-install-recommends \
      curl ca-certificates systemd tar gzip git

    if [[ "${COMPUTER_MCP_ENABLE_CERTBOT}" == "1" ]]; then
      apt-get install -y --no-install-recommends certbot || warn "certbot install failed"
    fi
    return
  fi

  if command_exists dnf; then
    dnf install -y curl ca-certificates systemd tar gzip git
    if [[ "${COMPUTER_MCP_ENABLE_CERTBOT}" == "1" ]]; then
      dnf install -y certbot || warn "certbot install failed"
    fi
    return
  fi

  if command_exists yum; then
    yum install -y curl ca-certificates systemd tar gzip git
    if [[ "${COMPUTER_MCP_ENABLE_CERTBOT}" == "1" ]]; then
      yum install -y certbot || warn "certbot install failed"
    fi
    return
  fi

  die "unsupported package manager (expected apt-get, dnf, or yum)"
}

resolve_release_api_url() {
  if [[ "${COMPUTER_MCP_VERSION}" == "latest" ]]; then
    printf 'https://api.github.com/repos/%s/releases/latest\n' "${COMPUTER_MCP_REPO}"
  else
    printf 'https://api.github.com/repos/%s/releases/tags/%s\n' \
      "${COMPUTER_MCP_REPO}" "${COMPUTER_MCP_VERSION}"
  fi
}

resolve_release_asset_url() {
  if [[ -n "${COMPUTER_MCP_ASSET_URL}" ]]; then
    printf '%s\n' "${COMPUTER_MCP_ASSET_URL}"
    return
  fi

  local metadata
  metadata="$(curl -fsSL "$(resolve_release_api_url)")" || return 1

  local asset_url
  asset_url="$(printf '%s' "${metadata}" \
    | tr '\n' ' ' \
    | sed 's/},{/},\n{/g' \
    | grep -Eo "\"browser_download_url\":\"[^\"]*${TARGET_TRIPLE}[^\"]*\\.tar\\.gz\"" \
    | head -n1 \
    | sed -E 's/"browser_download_url":"([^"]+)"/\1/' \
  )"

  [[ -n "${asset_url}" ]] || return 1
  printf '%s\n' "${asset_url}"
}

install_binaries_from_dir() {
  local src_dir="$1"
  [[ -x "${src_dir}/computer-mcp" ]] || die "missing executable ${src_dir}/computer-mcp"
  [[ -x "${src_dir}/computer-mcpd" ]] || die "missing executable ${src_dir}/computer-mcpd"

  install -d -m 0755 "${COMPUTER_MCP_INSTALL_DIR}"
  install -m 0755 "${src_dir}/computer-mcp" "${COMPUTER_MCP_INSTALL_DIR}/computer-mcp"
  install -m 0755 "${src_dir}/computer-mcpd" "${COMPUTER_MCP_INSTALL_DIR}/computer-mcpd"
}

install_binaries_from_release() {
  local asset_url
  asset_url="$(resolve_release_asset_url)" || return 1
  log "downloading release artifact: ${asset_url}"

  local archive="${TMP_DIR}/release.tar.gz"
  curl -fL "${asset_url}" -o "${archive}"
  tar -xzf "${archive}" -C "${TMP_DIR}"

  local cli_path
  cli_path="$(find "${TMP_DIR}" -type f -name computer-mcp -print -quit)"
  [[ -n "${cli_path}" ]] || return 1

  local extracted_dir
  extracted_dir="$(dirname "${cli_path}")"
  install_binaries_from_dir "${extracted_dir}"
}

install_rust_toolchain_if_needed() {
  if command_exists cargo && command_exists rustc; then
    return
  fi

  log "rust toolchain missing, installing via rustup"
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
}

install_binaries_from_source() {
  log "falling back to source build from ${COMPUTER_MCP_REPO}@${COMPUTER_MCP_SOURCE_REF}"
  install_rust_toolchain_if_needed

  local src_dir="${TMP_DIR}/source"
  git clone --depth 1 --branch "${COMPUTER_MCP_SOURCE_REF}" \
    "https://github.com/${COMPUTER_MCP_REPO}.git" "${src_dir}"

  (
    cd "${src_dir}"
    cargo build --release --bin computer-mcp --bin computer-mcpd
  )

  install_binaries_from_dir "${src_dir}/target/release"
}

ensure_dirs_and_config() {
  local config_dir
  config_dir="$(dirname "${COMPUTER_MCP_CONFIG_PATH}")"

  install -d -m 0750 "${config_dir}"
  install -d -m 0750 "${COMPUTER_MCP_STATE_DIR}"
  install -d -m 0750 "${COMPUTER_MCP_TLS_DIR}"

  if [[ ! -f "${COMPUTER_MCP_CONFIG_PATH}" ]]; then
    local api_key
    if command_exists openssl; then
      api_key="$(openssl rand -hex 24)"
    else
      api_key="$(tr -dc 'A-Za-z0-9' </dev/urandom | head -c 48)"
    fi

    umask 077
    cat >"${COMPUTER_MCP_CONFIG_PATH}" <<EOF
bind_host = "0.0.0.0"
bind_port = 443
api_key = "${api_key}"
tls_mode = "auto"
tls_cert_path = "${COMPUTER_MCP_TLS_DIR}/cert.pem"
tls_key_path = "${COMPUTER_MCP_TLS_DIR}/key.pem"
max_sessions = 64
default_exec_timeout_ms = 7200000
max_exec_timeout_ms = 7200000
default_exec_yield_time_ms = 10000
default_write_yield_time_ms = 10000
max_output_chars = 200000
EOF
    log "created config at ${COMPUTER_MCP_CONFIG_PATH}"
  fi

  chmod 0600 "${COMPUTER_MCP_CONFIG_PATH}"
}

run_cli_install() {
  local cli="${COMPUTER_MCP_INSTALL_DIR}/computer-mcp"
  [[ -x "${cli}" ]] || die "computer-mcp not installed at ${cli}"
  "${cli}" --config "${COMPUTER_MCP_CONFIG_PATH}" install
}

detect_public_ip() {
  local ip=""
  ip="$(curl -fsS --max-time 5 https://api.ipify.org || true)"
  if [[ -z "${ip}" ]]; then
    ip="<public_ip>"
  fi
  printf '%s\n' "${ip}"
}

print_next_steps() {
  local ip
  ip="$(detect_public_ip)"
  cat <<EOF

Install complete.

Next steps:
  1. computer-mcp --config "${COMPUTER_MCP_CONFIG_PATH}" set-key "<strong-random-key>"
  2. computer-mcp --config "${COMPUTER_MCP_CONFIG_PATH}" tls setup
  3. computer-mcp --config "${COMPUTER_MCP_CONFIG_PATH}" start
  4. computer-mcp --config "${COMPUTER_MCP_CONFIG_PATH}" show-url --host "${ip}"

Verify:
  - computer-mcp --config "${COMPUTER_MCP_CONFIG_PATH}" status
  - curl -k "https://${ip}/health"
  - MCP URL shape: https://${ip}/mcp?key=<redacted>
EOF
}

main() {
  need_root
  detect_platform
  install_prerequisites

  TMP_DIR="$(mktemp -d)"
  trap cleanup EXIT

  if [[ -n "${COMPUTER_MCP_BINARY_SOURCE_DIR}" ]]; then
    log "installing binaries from COMPUTER_MCP_BINARY_SOURCE_DIR=${COMPUTER_MCP_BINARY_SOURCE_DIR}"
    install_binaries_from_dir "${COMPUTER_MCP_BINARY_SOURCE_DIR}"
  elif ! install_binaries_from_release; then
    warn "release artifact install failed; attempting source build fallback"
    install_binaries_from_source
  fi

  ensure_dirs_and_config
  run_cli_install
  print_next_steps
}

main "$@"
