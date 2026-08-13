#!/usr/bin/env bash
set -euo pipefail

NS="@NAMESPACE@"
ROOT_IF="@ROOT_INTERFACE@"
AGENT_IF="@AGENT_INTERFACE@"
ROOT_ADDR="@ROOT_ADDRESS@"
AGENT_ADDR="@AGENT_ADDRESS@"
ROOT_GATEWAY="@ROOT_GATEWAY@"
POLICY_VERSION="@POLICY_VERSION@"
FILTER_TABLE="zodex_local_filter"
NAT_TABLE="zodex_local_nat"
RUNTIME_DIR="/run/zodex-local-network"
DNS_DIR="/etc/netns/${NS}"
DNS_FILE="${DNS_DIR}/resolv.conf"
NON_PUBLIC_CIDRS="@NON_PUBLIC_CIDRS_SPACE@"
DEFAULT_DNS="@DEFAULT_DNS_SPACE@"

fail() {
  printf 'zodex-local-network: %s\n' "$*" >&2
  exit 1
}

require_root() {
  [[ "$(id -u)" == "0" ]] || fail "must run as root"
}

require_tools() {
  local tool
  for tool in ip nft sysctl python3; do
    command -v "$tool" >/dev/null 2>&1 || fail "required tool is missing: ${tool}"
  done
}

provider_interface() {
  local iface
  iface="$(ip -4 route show default | awk 'NR == 1 { for (i=1; i<=NF; i++) if ($i == "dev") { print $(i+1); exit } }')"
  [[ -n "$iface" ]] || fail "no IPv4 default-route interface is available"
  [[ "$iface" != "$ROOT_IF" && "$iface" != "$AGENT_IF" ]] || fail "default route points at the Zodex veth"
  case "$iface" in
    *[!A-Za-z0-9_.:-]*) fail "default-route interface contains unsupported characters" ;;
  esac
  printf '%s\n' "$iface"
}

write_resolver_config() {
  install -d -m 0755 -o root -g root "$DNS_DIR"
  ZODEX_LOCAL_NON_PUBLIC_CIDRS="$NON_PUBLIC_CIDRS" \
  ZODEX_LOCAL_DEFAULT_DNS="$DEFAULT_DNS" \
  python3 - "$DNS_FILE" <<'PY'
import ipaddress
import os
from pathlib import Path
import sys

output = Path(sys.argv[1])
blocked = [ipaddress.ip_network(item) for item in os.environ["ZODEX_LOCAL_NON_PUBLIC_CIDRS"].split()]
defaults = os.environ["ZODEX_LOCAL_DEFAULT_DNS"].split()
selected = []
try:
    lines = Path("/etc/resolv.conf").read_text(encoding="utf-8").splitlines()
except OSError:
    lines = []
for line in lines:
    fields = line.split()
    if len(fields) < 2 or fields[0] != "nameserver":
        continue
    try:
        address = ipaddress.ip_address(fields[1])
    except ValueError:
        continue
    if address.version != 4 or any(address in network for network in blocked):
        continue
    text = str(address)
    if text not in selected:
        selected.append(text)
    if len(selected) == 3:
        break
if not selected:
    selected = defaults[:3]
output.write_text("".join(f"nameserver {address}\n" for address in selected), encoding="utf-8")
PY
  chown root:root "$DNS_FILE"
  chmod 0644 "$DNS_FILE"
}

remove_owned_policy() {
  nft list table inet "$FILTER_TABLE" >/dev/null 2>&1 && nft delete table inet "$FILTER_TABLE"
  nft list table ip "$NAT_TABLE" >/dev/null 2>&1 && nft delete table ip "$NAT_TABLE"
}

remove_owned_topology() {
  if ip netns list | awk '{print $1}' | grep -Fxq "$NS"; then
    [[ -z "$(ip netns pids "$NS")" ]] || fail "network namespace still has running processes"
    ip netns delete "$NS"
  fi
  if ip link show "$ROOT_IF" >/dev/null 2>&1; then
    ip link delete "$ROOT_IF"
  fi
}

install_topology() {
  ip netns add "$NS"
  ip link add "$ROOT_IF" type veth peer name "$AGENT_IF"
  ip link set "$AGENT_IF" netns "$NS"

  ip addr add "$ROOT_ADDR" dev "$ROOT_IF"
  ip link set "$ROOT_IF" up
  ip -n "$NS" addr add "$AGENT_ADDR" dev "$AGENT_IF"
  ip -n "$NS" link set lo up
  ip -n "$NS" link set "$AGENT_IF" up
  ip -n "$NS" route add default via "$ROOT_GATEWAY" dev "$AGENT_IF"

  sysctl -q -w net.ipv4.ip_forward=1
  sysctl -q -w "net.ipv6.conf.${ROOT_IF}.disable_ipv6=1"
  ip netns exec "$NS" sysctl -q -w net.ipv4.ip_forward=0
  ip netns exec "$NS" sysctl -q -w net.ipv6.conf.all.disable_ipv6=1
  ip netns exec "$NS" sysctl -q -w net.ipv6.conf.default.disable_ipv6=1
  ip netns exec "$NS" sysctl -q -w "net.ipv6.conf.${AGENT_IF}.disable_ipv6=1"
}

install_policy() {
  local provider_if="$1"
  nft -f - <<EOF_NFT
add table inet ${FILTER_TABLE}
add set inet ${FILTER_TABLE} non_public_v4 { type ipv4_addr; flags interval; elements = { @NON_PUBLIC_CIDRS_NFT@ }; }
add chain inet ${FILTER_TABLE} input { type filter hook input priority 0; policy accept; }
add rule inet ${FILTER_TABLE} input iifname "${ROOT_IF}" drop
add chain inet ${FILTER_TABLE} forward { type filter hook forward priority 0; policy accept; }
add rule inet ${FILTER_TABLE} forward iifname "${ROOT_IF}" ip daddr @non_public_v4 drop
add rule inet ${FILTER_TABLE} forward iifname "${ROOT_IF}" meta nfproto ipv4 accept
add rule inet ${FILTER_TABLE} forward iifname "${ROOT_IF}" drop
add rule inet ${FILTER_TABLE} forward oifname "${ROOT_IF}" ct state established,related accept
add rule inet ${FILTER_TABLE} forward oifname "${ROOT_IF}" drop
add table ip ${NAT_TABLE}
add chain ip ${NAT_TABLE} postrouting { type nat hook postrouting priority srcnat; policy accept; }
add rule ip ${NAT_TABLE} postrouting ip saddr @AGENT_IPV4@ oifname "${provider_if}" masquerade
EOF_NFT
}

capture_policy_snapshot() {
  install -d -m 0755 -o root -g root "$RUNTIME_DIR"
  nft list table inet "$FILTER_TABLE" > "${RUNTIME_DIR}/filter.nft"
  nft list table ip "$NAT_TABLE" > "${RUNTIME_DIR}/nat.nft"
  printf '%s\n' "$POLICY_VERSION" > "${RUNTIME_DIR}/policy-version"
  chmod 0444 "${RUNTIME_DIR}/filter.nft" "${RUNTIME_DIR}/nat.nft" "${RUNTIME_DIR}/policy-version"
}

verify_topology() {
  [[ -r "/run/netns/${NS}" ]] || fail "named network namespace is missing"
  ip link show "$ROOT_IF" >/dev/null 2>&1 || fail "root veth is missing"
  ip -n "$NS" link show "$AGENT_IF" >/dev/null 2>&1 || fail "agent veth is missing"
  [[ "$(ip -n "$NS" -o link show | awk 'END {print NR}')" == "2" ]] || fail "agent namespace has unexpected network interfaces"
  ! ip -n "$NS" link show eth0 >/dev/null 2>&1 || fail "provider interface leaked into agent namespace"
  ip -4 -o addr show dev "$ROOT_IF" | awk '{print $4}' | grep -Fxq "$ROOT_ADDR" || fail "root veth address drifted"
  ip -n "$NS" -4 -o addr show dev "$AGENT_IF" | awk '{print $4}' | grep -Fxq "$AGENT_ADDR" || fail "agent veth address drifted"
  ip -n "$NS" route show default | grep -Fxq "default via ${ROOT_GATEWAY} dev ${AGENT_IF}" || fail "agent default route drifted"
  [[ -z "$(ip -n "$NS" -6 addr show scope global)" ]] || fail "global IPv6 is enabled in agent namespace"
  [[ "$(ip netns exec "$NS" sysctl -n net.ipv6.conf.all.disable_ipv6)" == "1" ]] || fail "IPv6 disablement drifted"
  [[ "$(ip netns exec "$NS" sysctl -n net.ipv4.ip_forward)" == "0" ]] || fail "agent namespace forwarding unexpectedly enabled"
  [[ -s "$DNS_FILE" ]] || fail "namespace resolver configuration is missing"
}

verify_policy() {
  [[ -r "${RUNTIME_DIR}/policy-version" ]] || fail "policy version marker is missing"
  [[ "$(cat "${RUNTIME_DIR}/policy-version")" == "$POLICY_VERSION" ]] || fail "policy version drifted"
  cmp -s <(nft list table inet "$FILTER_TABLE") "${RUNTIME_DIR}/filter.nft" || fail "filter policy drifted"
  cmp -s <(nft list table ip "$NAT_TABLE") "${RUNTIME_DIR}/nat.nft" || fail "NAT policy drifted"
}

verify_all() {
  verify_topology
  verify_policy
}

reconcile() {
  local provider_if
  provider_if="$(provider_interface)"
  remove_owned_policy
  remove_owned_topology
  install_topology
  write_resolver_config
  install_policy "$provider_if"
  capture_policy_snapshot
  verify_all
}

require_root
require_tools
case "${1:-reconcile}" in
  reconcile) reconcile ;;
  verify) verify_all ;;
  *) fail "usage: $0 [reconcile|verify]" ;;
esac
