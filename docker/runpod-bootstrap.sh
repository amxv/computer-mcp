#!/usr/bin/env bash
set -euo pipefail

CONFIG_PATH="${COMPUTER_MCP_CONFIG_PATH:-/etc/computer-mcp/config.toml}"
READER_KEY_PATH="${COMPUTER_MCP_READER_KEY_PATH:-/etc/computer-mcp/reader/private-key.pem}"
PUBLISHER_KEY_PATH="${COMPUTER_MCP_PUBLISHER_KEY_PATH:-/etc/computer-mcp/publisher/private-key.pem}"
BOOT_LOG_PREFIX="[computer-mcp runpod]"
SERVICE_GROUP="${COMPUTER_MCP_SERVICE_GROUP:-computer-mcp}"
AGENT_USER="${COMPUTER_MCP_AGENT_USER:-computer-mcp-agent}"
AGENT_HOME="${COMPUTER_MCP_AGENT_HOME:-/home/${AGENT_USER}}"
AGENT_SHELL="${COMPUTER_MCP_AGENT_SHELL:-/bin/bash}"
PUBLISHER_USER="${COMPUTER_MCP_PUBLISHER_USER:-computer-mcp-publisher}"

log() {
  printf '%s %s\n' "${BOOT_LOG_PREFIX}" "$*"
}

ensure_process_mode_accounts() {
  if ! getent group "${SERVICE_GROUP}" >/dev/null 2>&1; then
    groupadd --system "${SERVICE_GROUP}"
    log "created service group ${SERVICE_GROUP}"
  fi

  ensure_agent_user
  ensure_publisher_user
  ensure_agent_dev_environment
  ensure_agent_ssh_access
}

ensure_agent_user() {
  if ! id -u "${AGENT_USER}" >/dev/null 2>&1; then
    useradd --system \
      --create-home \
      --home-dir "${AGENT_HOME}" \
      --shell "${AGENT_SHELL}" \
      --gid "${SERVICE_GROUP}" \
      "${AGENT_USER}"
    log "created agent user ${AGENT_USER}"
    return
  fi

  local current_home
  current_home="$(getent passwd "${AGENT_USER}" | cut -d: -f6)"
  local current_shell
  current_shell="$(getent passwd "${AGENT_USER}" | cut -d: -f7)"

  if [[ "${current_home}" != "${AGENT_HOME}" ]]; then
    usermod --home "${AGENT_HOME}" "${AGENT_USER}"
  fi

  if [[ "${current_shell}" != "${AGENT_SHELL}" ]]; then
    usermod --shell "${AGENT_SHELL}" "${AGENT_USER}"
  fi
}

ensure_publisher_user() {
  if id -u "${PUBLISHER_USER}" >/dev/null 2>&1; then
    return
  fi

  useradd --system \
    --no-create-home \
    --home-dir /nonexistent \
    --shell /usr/sbin/nologin \
    --gid "${SERVICE_GROUP}" \
    "${PUBLISHER_USER}"
  log "created publisher user ${PUBLISHER_USER}"
}

ensure_agent_dev_environment() {
  install -d -m 0750 -o "${AGENT_USER}" -g "${SERVICE_GROUP}" \
    "${AGENT_HOME}" \
    "${AGENT_HOME}/.bun" \
    "${AGENT_HOME}/.bun/bin" \
    "${AGENT_HOME}/.cargo" \
    "${AGENT_HOME}/.cargo/bin" \
    "${AGENT_HOME}/.local" \
    "${AGENT_HOME}/.local/bin" \
    "${AGENT_HOME}/.npm-global" \
    "${AGENT_HOME}/.npm-global/bin" \
    "${AGENT_HOME}/go" \
    "${AGENT_HOME}/go/bin"

  if [[ -d /workspace ]]; then
    chown "${AGENT_USER}:${SERVICE_GROUP}" /workspace || true
    chmod 0775 /workspace || true
  fi
}

collect_agent_ssh_keys() {
  local key_lines=()

  if [[ -n "${SSH_PUBLIC_KEY:-}" ]]; then
    key_lines+=("${SSH_PUBLIC_KEY}")
  fi

  if [[ -n "${PUBLIC_KEY:-}" ]]; then
    key_lines+=("${PUBLIC_KEY}")
  fi

  if [[ -f /root/.ssh/authorized_keys ]]; then
    while IFS= read -r line; do
      key_lines+=("${line}")
    done < /root/.ssh/authorized_keys
  fi

  if [[ ${#key_lines[@]} -eq 0 ]]; then
    return
  fi

  printf '%s\n' "${key_lines[@]}" | awk 'NF && !seen[$0]++'
}

ensure_agent_ssh_access() {
  local ssh_dir="${AGENT_HOME}/.ssh"
  local authorized_keys="${ssh_dir}/authorized_keys"
  local key_material
  key_material="$(collect_agent_ssh_keys || true)"

  if [[ -n "${key_material}" ]]; then
    install -d -m 0700 -o "${AGENT_USER}" -g "${SERVICE_GROUP}" "${ssh_dir}"
    printf '%s\n' "${key_material}" > "${authorized_keys}"
    chown "${AGENT_USER}:${SERVICE_GROUP}" "${authorized_keys}" || true
    chmod 0600 "${authorized_keys}" || true
    log "installed SSH public key for ${AGENT_USER} access"
  fi

  ensure_agent_account_accepts_pubkey
}

ensure_agent_account_accepts_pubkey() {
  local shadow_entry
  shadow_entry="$(getent shadow "${AGENT_USER}" | cut -d: -f2 || true)"

  case "${shadow_entry}" in
    '!'*|'*'|'')
      local password_hash
      password_hash="$(openssl rand -base64 48 | tr -d '\n' | openssl passwd -6 -stdin)"
      usermod -p "${password_hash}" "${AGENT_USER}"
      log "set random password hash for ${AGENT_USER} to allow SSH public-key login"
      ;;
  esac
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

ensure_service_path_permissions() {
  local config_dir
  config_dir="$(dirname "${CONFIG_PATH}")"
  local reader_dir
  reader_dir="$(dirname "${READER_KEY_PATH}")"
  local publisher_dir
  publisher_dir="$(dirname "${PUBLISHER_KEY_PATH}")"
  local tls_dir="/var/lib/computer-mcp/tls"

  for dir_path in "${config_dir}" "${reader_dir}" "${publisher_dir}" "${tls_dir}"; do
    if [[ -d "${dir_path}" ]]; then
      chgrp "${SERVICE_GROUP}" "${dir_path}" || true
      chmod 0750 "${dir_path}" || true
    fi
  done

  if [[ -f "${CONFIG_PATH}" ]]; then
    chown "root:${SERVICE_GROUP}" "${CONFIG_PATH}" || true
    chmod 0640 "${CONFIG_PATH}" || true
  fi

  if [[ -f "${READER_KEY_PATH}" ]]; then
    chown "root:${SERVICE_GROUP}" "${READER_KEY_PATH}" || true
    chmod 0640 "${READER_KEY_PATH}" || true
  fi

  if [[ -f "${PUBLISHER_KEY_PATH}" ]]; then
    chown "${PUBLISHER_USER}:${SERVICE_GROUP}" "${PUBLISHER_KEY_PATH}" || true
    chmod 0600 "${PUBLISHER_KEY_PATH}" || true
  fi

  if [[ -f "${tls_dir}/cert.pem" ]]; then
    chown "root:${SERVICE_GROUP}" "${tls_dir}/cert.pem" || true
    chmod 0644 "${tls_dir}/cert.pem" || true
  fi

  if [[ -f "${tls_dir}/key.pem" ]]; then
    chown "root:${SERVICE_GROUP}" "${tls_dir}/key.pem" || true
    chmod 0640 "${tls_dir}/key.pem" || true
  fi
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
    chgrp "${SERVICE_GROUP}" "${CONFIG_PATH}" || true
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
agent_user = "${AGENT_USER}"
publisher_user = "${PUBLISHER_USER}"
service_group = "${SERVICE_GROUP}"
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
  chgrp "${SERVICE_GROUP}" "${CONFIG_PATH}" || true
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

derive_public_host() {
  if [[ -n "${COMPUTER_MCP_PUBLIC_HOST:-}" ]]; then
    printf '%s\n' "${COMPUTER_MCP_PUBLIC_HOST}"
    return
  fi

  if [[ -n "${RUNPOD_POD_ID:-}" ]]; then
    local http_bind_port="${COMPUTER_MCP_HTTP_BIND_PORT:-8080}"
    printf '%s-%s.proxy.runpod.net\n' "${RUNPOD_POD_ID}" "${http_bind_port}"
  fi
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

  ensure_service_path_permissions

  if ! computer-mcp start; then
    ensure_service_path_permissions
    log "computer-mcp start failed; pod will stay up for SSH/debugging"
    return
  fi

  local public_host
  public_host="$(derive_public_host || true)"
  if [[ -n "${public_host}" ]]; then
    computer-mcp show-url --host "${public_host}" || true
  fi

  ensure_service_path_permissions
  computer-mcp status || true
}

main() {
  ensure_process_mode_accounts
  write_secret_file_from_env "COMPUTER_MCP_READER_PRIVATE_KEY" "${READER_KEY_PATH}" "root:${SERVICE_GROUP}"
  write_secret_file_from_env "COMPUTER_MCP_PUBLISHER_PRIVATE_KEY" "${PUBLISHER_KEY_PATH}" "${PUBLISHER_USER}:${SERVICE_GROUP}"
  bootstrap_computer_mcp_config
  ensure_service_path_permissions
  start_computer_mcp_if_ready
}

main "$@"
