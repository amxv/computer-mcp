#!/usr/bin/env bash
set -euo pipefail

if [[ -x /start.sh ]]; then
  /start.sh &
  sleep "${RUNPOD_BASE_START_DELAY_SECS:-2}"
fi

if ! /usr/local/bin/computer-mcp-container-bootstrap; then
  echo "[computer-mcp runpod] bootstrap failed; pod will stay up for SSH/debugging"
fi

exec tail -f /dev/null
