#!/usr/bin/env bash
set -euo pipefail

mkdir -p /run/sshd /workspace

if [[ ! -f /etc/ssh/ssh_host_ed25519_key ]]; then
  ssh-keygen -A
fi

/usr/sbin/sshd

if [[ "$#" -gt 0 ]]; then
  exec "$@"
fi

exec sleep infinity
