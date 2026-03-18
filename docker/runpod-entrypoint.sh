#!/usr/bin/env bash
set -euo pipefail

CONFIG_PATH="${COMPUTER_MCP_CONFIG_PATH:-/etc/computer-mcp/config.toml}"
READER_KEY_PATH="${COMPUTER_MCP_READER_KEY_PATH:-/etc/computer-mcp/reader/private-key.pem}"
PUBLISHER_KEY_PATH="${COMPUTER_MCP_PUBLISHER_KEY_PATH:-/etc/computer-mcp/publisher/private-key.pem}"
BOOT_LOG_PREFIX="[computer-mcp container]"

log() {
  printf '%s %s\n' "${BOOT_LOG_PREFIX}" "$*"
}

write_authorized_keys() {
  local ssh_key="${SSH_PUBLIC_KEY:-${PUBLIC_KEY:-}}"

  if [[ -z "${ssh_key}" ]]; then
    log "no PUBLIC_KEY or SSH_PUBLIC_KEY provided; skipping SSH key injection"
    return
  fi

  mkdir -p /root/.ssh
  chmod 700 /root/.ssh

  if [[ -f /root/.ssh/authorized_keys ]] && grep -Fqx "${ssh_key}" /root/.ssh/authorized_keys; then
    log "ssh key already present in /root/.ssh/authorized_keys"
  else
    printf '%s\n' "${ssh_key}" >> /root/.ssh/authorized_keys
    log "installed SSH public key for root access"
  fi

  chmod 600 /root/.ssh/authorized_keys
}

ensure_ssh_host_keys() {
  mkdir -p /run/sshd

  if [[ ! -f /etc/ssh/ssh_host_ed25519_key ]]; then
    ssh-keygen -A
  fi
}

start_sshd() {
  if pgrep -x sshd >/dev/null 2>&1; then
    log "sshd already running"
    return
  fi

  /usr/sbin/sshd
  log "started sshd"
}

export_env_vars_for_shells() {
  local env_file="/etc/profile.d/runpod-env.sh"

  printenv \
    | grep -E '^[A-Z_][A-Z0-9_]*=' \
    | grep -vE '^(PUBLIC_KEY|SSH_PUBLIC_KEY)=' \
    | awk -F= '
        {
          key = $1
          value = substr($0, index($0, "=") + 1)
          gsub(/["\\]/, "\\\\&", value)
          printf("export %s=\"%s\"\n", key, value)
        }
      ' > "${env_file}"

  chmod 0644 "${env_file}"
}

write_secret_file_from_env() {
  local env_name="$1"
  local target_path="$2"
  local owner_spec="$3"

  local value="${!env_name:-}"
  if [[ -z "${value}" ]]; then
    return
  fi

  install -d -m 0750 "$(dirname "${target_path}")"
  printf '%s\n' "${value}" > "${target_path}"
  chmod 0600 "${target_path}"
  chown "${owner_spec}" "${target_path}" || true
  log "wrote ${env_name} to ${target_path}"
}

has_auto_config_inputs() {
  [[ -n "${COMPUTER_MCP_READER_APP_ID:-}" ]] \
    && [[ -n "${COMPUTER_MCP_READER_INSTALLATION_ID:-}" ]] \
    && [[ -n "${COMPUTER_MCP_PUBLISHER_APP_ID:-}" ]] \
    && [[ -n "${COMPUTER_MCP_PUBLISHER_INSTALLATION_ID:-}" ]] \
    && [[ -n "${COMPUTER_MCP_PUBLISHER_TARGET_REPO:-}" ]]
}

has_full_config_input() {
  [[ -n "${COMPUTER_MCP_CONFIG_TOML:-}" ]]
}

install_if_needed() {
  if [[ -x /usr/local/bin/computer-mcp ]]; then
    computer-mcp install
    return
  fi

  log "computer-mcp binary missing from image"
  return 1
}

bootstrap_computer_mcp_config() {
  install_if_needed

  if [[ -f "${CONFIG_PATH}" ]] && [[ "${COMPUTER_MCP_FORCE_RECONFIGURE:-0}" != "1" ]] && ! has_auto_config_inputs && ! has_full_config_input; then
    log "existing computer-mcp config found at ${CONFIG_PATH}; leaving it unchanged"
    return
  fi

  if has_full_config_input; then
    install -d -m 0750 "$(dirname "${CONFIG_PATH}")"
    printf '%s\n' "${COMPUTER_MCP_CONFIG_TOML}" > "${CONFIG_PATH}"
    chmod 0640 "${CONFIG_PATH}"
    chgrp computer-mcp "${CONFIG_PATH}" || true
    log "wrote computer-mcp config from COMPUTER_MCP_CONFIG_TOML to ${CONFIG_PATH}"
    return
  fi

  if ! has_auto_config_inputs; then
    log "computer-mcp auto-config not attempted; required env vars are missing"
    return
  fi

  local api_key="${COMPUTER_MCP_API_KEY:-}"
  if [[ -z "${api_key}" ]]; then
    api_key="$(openssl rand -hex 24)"
  fi

  local http_bind_port="${COMPUTER_MCP_HTTP_BIND_PORT:-8080}"
  local default_base="${COMPUTER_MCP_PUBLISHER_DEFAULT_BASE:-main}"

  install -d -m 0750 "$(dirname "${CONFIG_PATH}")"
  cat > "${CONFIG_PATH}" <<EOF
bind_host = "0.0.0.0"
bind_port = 443
http_bind_port = ${http_bind_port}
api_key = "${api_key}"
tls_mode = "self_signed"
tls_cert_path = "/var/lib/computer-mcp/tls/cert.pem"
tls_key_path = "/var/lib/computer-mcp/tls/key.pem"
max_sessions = 64
default_exec_timeout_ms = 7200000
max_exec_timeout_ms = 7200000
default_exec_yield_time_ms = 10000
default_write_yield_time_ms = 10000
max_output_chars = 200000
reader_app_id = ${COMPUTER_MCP_READER_APP_ID}
reader_installation_id = ${COMPUTER_MCP_READER_INSTALLATION_ID}
reader_private_key_path = "${READER_KEY_PATH}"
publisher_socket_path = "/var/lib/computer-mcp/publisher/run/computer-mcp-prd.sock"
publisher_private_key_path = "${PUBLISHER_KEY_PATH}"
publisher_app_id = ${COMPUTER_MCP_PUBLISHER_APP_ID}
agent_user = "computer-mcp-agent"
publisher_user = "computer-mcp-publisher"
service_group = "computer-mcp"
publisher_branch_prefix = "agent"
publisher_max_bundle_bytes = 8388608
publisher_max_title_chars = 240
publisher_max_body_chars = 16000

[[publisher_targets]]
id = "${COMPUTER_MCP_PUBLISHER_TARGET_REPO}"
repo = "${COMPUTER_MCP_PUBLISHER_TARGET_REPO}"
default_base = "${default_base}"
installation_id = ${COMPUTER_MCP_PUBLISHER_INSTALLATION_ID}
EOF

  chmod 0640 "${CONFIG_PATH}"
  chgrp computer-mcp "${CONFIG_PATH}" || true
  log "wrote computer-mcp config to ${CONFIG_PATH}"
}

config_is_startable() {
  [[ -f "${CONFIG_PATH}" ]] || return 1

  grep -Eq '^reader_app_id = [1-9][0-9]*$' "${CONFIG_PATH}" || return 1
  grep -Eq '^reader_installation_id = [1-9][0-9]*$' "${CONFIG_PATH}" || return 1
  grep -Eq '^publisher_app_id = [1-9][0-9]*$' "${CONFIG_PATH}" || return 1
  grep -Eq '^installation_id = [1-9][0-9]*$' "${CONFIG_PATH}" || return 1

  [[ -f "${READER_KEY_PATH}" ]] || return 1
  [[ -f "${PUBLISHER_KEY_PATH}" ]] || return 1
}

start_computer_mcp_if_ready() {
  local auto_start="${COMPUTER_MCP_AUTO_START:-1}"
  if [[ "${auto_start}" != "1" ]]; then
    log "COMPUTER_MCP_AUTO_START=${auto_start}; skipping automatic service start"
    return
  fi

  if [[ ! -f "${CONFIG_PATH}" ]]; then
    log "computer-mcp config missing at ${CONFIG_PATH}; skipping automatic start"
    return
  fi

  if ! config_is_startable; then
    log "computer-mcp config or key files are incomplete; skipping automatic start"
    return
  fi

  if ! computer-mcp start; then
    log "computer-mcp start failed; container will stay up for SSH/debugging"
    return
  fi

  if [[ -n "${COMPUTER_MCP_PUBLIC_HOST:-}" ]]; then
    computer-mcp show-url --host "${COMPUTER_MCP_PUBLIC_HOST}" || true
  fi

  computer-mcp status || true
}

main() {
  write_authorized_keys
  ensure_ssh_host_keys
  export_env_vars_for_shells
  start_sshd

  write_secret_file_from_env "COMPUTER_MCP_READER_PRIVATE_KEY" "${READER_KEY_PATH}" "root:computer-mcp"
  write_secret_file_from_env "COMPUTER_MCP_PUBLISHER_PRIVATE_KEY" "${PUBLISHER_KEY_PATH}" "computer-mcp-publisher:computer-mcp"

  bootstrap_computer_mcp_config
  start_computer_mcp_if_ready

  if [[ "$#" -gt 0 ]]; then
    exec "$@"
  fi

  exec tail -f /dev/null
}

main "$@"
