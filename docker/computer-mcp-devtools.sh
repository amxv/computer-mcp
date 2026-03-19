#!/usr/bin/env bash

set_path_prepend() {
  local dir="$1"
  if [[ -z "${dir}" ]] || [[ ! -d "${dir}" ]]; then
    return
  fi

  case ":${PATH:-}:" in
    *":${dir}:"*) ;;
    *) PATH="${dir}${PATH:+:${PATH}}" ;;
  esac
}

set_path_prepend "/usr/local/go/bin"

if [[ -n "${HOME:-}" ]] && [[ "${HOME}" != "/nonexistent" ]]; then
  export CARGO_HOME="${CARGO_HOME:-${HOME}/.cargo}"
  export BUN_INSTALL="${BUN_INSTALL:-${HOME}/.bun}"
  export PIPX_HOME="${PIPX_HOME:-${HOME}/.local/pipx}"
  export PIPX_BIN_DIR="${PIPX_BIN_DIR:-${HOME}/.local/bin}"
  export GOPATH="${GOPATH:-${HOME}/go}"
  export NPM_CONFIG_PREFIX="${NPM_CONFIG_PREFIX:-${HOME}/.npm-global}"

  set_path_prepend "${BUN_INSTALL}/bin"
  set_path_prepend "${CARGO_HOME}/bin"
  set_path_prepend "${HOME}/.local/bin"
  set_path_prepend "${GOPATH}/bin"
  set_path_prepend "${NPM_CONFIG_PREFIX}/bin"
fi

export RUSTUP_HOME="${RUSTUP_HOME:-/usr/local/rustup}"
set_path_prepend "/usr/local/bin"

export PATH
