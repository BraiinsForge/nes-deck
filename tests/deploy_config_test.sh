#!/usr/bin/env bash

set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
fixture=$(mktemp -d /tmp/nes-deck-config-test.XXXXXX)
trap 'rm -rf "$fixture"' EXIT INT TERM HUP
config=$fixture/deck.conf
password='configured-test-password'

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

printf '%s\n%s\n%s\n%s\n%s\n%s\n' \
  'root@192.0.2.50' '198.51.100.11' '198.51.100.0/24' \
  '198.51.100.1' "$password" "$password" |
  "$root/ops/configure-deck.sh" "$config" >/dev/null

[[ $(stat -c %a "$config") == 600 ]] || fail 'configuration is private'
grep -qx 'DECK_SSH_TARGET=root@192.0.2.50' "$config" ||
  fail 'SSH target was not stored'
grep -qx 'DECK_WIREGUARD_ADDRESS=198.51.100.11' "$config" ||
  fail 'WireGuard address was not stored'
grep -qx 'DECK_WIREGUARD_ROUTE=198.51.100.0/24' "$config" ||
  fail 'WireGuard route was not stored'
grep -qx 'DECK_WIREGUARD_HEALTH_ADDRESS=198.51.100.1' "$config" ||
  fail 'WireGuard health address was not stored'
grep -qx "ROM_UPLOADER_PASSWORD=$password" "$config" ||
  fail 'uploader password was not stored'
"$root/ops/deploy.sh" --config "$config" --check-config |
  grep -qx 'Deck configuration is valid for root@192.0.2.50 at 198.51.100.11' ||
  fail 'valid configuration was rejected'

chmod 0644 "$config"
if "$root/ops/deploy.sh" --config "$config" --check-config >/dev/null 2>&1; then
  fail 'public configuration was accepted'
fi
chmod 0600 "$config"

sed -i 's/configured-test-password/short/' "$config"
if "$root/ops/deploy.sh" --config "$config" --check-config >/dev/null 2>&1; then
  fail 'short uploader password was accepted'
fi

sed -i 's/short/configured-test-password/' "$config"
sed -i 's/198\.51\.100\.11/127.0.0.1/' "$config"
if "$root/ops/deploy.sh" --config "$config" --check-config >/dev/null 2>&1; then
  fail 'loopback WireGuard address was accepted for a Deck'
fi

sed -i 's/127\.0\.0\.1/198.51.100.11/' "$config"
"$root/ops/deploy.sh" --config "$config" --check-config root@192.0.2.7 |
  grep -qx 'Deck configuration is valid for root@192.0.2.7 at 198.51.100.11' ||
  fail 'temporary SSH target override was rejected'

assert_rejected() {
  local name=$1
  local expected=$2
  local candidate=$fixture/$name.conf
  shift 2
  printf '%s\n' "$@" >"$candidate"
  chmod 0600 "$candidate"
  local output
  if output=$("$root/ops/deploy.sh" --config "$candidate" \
      --check-config 2>&1); then
    fail "$name configuration was accepted"
  fi
  grep -Fq "$expected" <<<"$output" ||
    fail "$name configuration did not explain its error"
}

assert_rejected malformed 'must have the form KEY=VALUE' \
  'this is not a setting'
assert_rejected unknown 'is not supported: EXTRA_SETTING' \
  'DECK_SSH_TARGET=root@192.0.2.50' \
  'DECK_WIREGUARD_ADDRESS=198.51.100.11' \
  'DECK_WIREGUARD_ROUTE=198.51.100.0/24' \
  'DECK_WIREGUARD_HEALTH_ADDRESS=198.51.100.1' \
  'ROM_UPLOADER_PASSWORD=configured-test-password' \
  'EXTRA_SETTING=true'
assert_rejected duplicate 'repeats DECK_SSH_TARGET' \
  'DECK_SSH_TARGET=root@192.0.2.50' \
  'DECK_SSH_TARGET=root@192.0.2.51' \
  'DECK_WIREGUARD_ADDRESS=198.51.100.11' \
  'DECK_WIREGUARD_ROUTE=198.51.100.0/24' \
  'DECK_WIREGUARD_HEALTH_ADDRESS=198.51.100.1' \
  'ROM_UPLOADER_PASSWORD=configured-test-password'
assert_rejected missing-password 'is missing ROM_UPLOADER_PASSWORD' \
  'DECK_SSH_TARGET=root@192.0.2.50' \
  'DECK_WIREGUARD_ADDRESS=198.51.100.11' \
  'DECK_WIREGUARD_ROUTE=198.51.100.0/24' \
  'DECK_WIREGUARD_HEALTH_ADDRESS=198.51.100.1'
assert_rejected noncanonical-wireguard 'must be a canonical unicast IPv4 address' \
  'DECK_SSH_TARGET=root@192.0.2.50' \
  'DECK_WIREGUARD_ADDRESS=198.51.100.011' \
  'DECK_WIREGUARD_ROUTE=198.51.100.0/24' \
  'DECK_WIREGUARD_HEALTH_ADDRESS=198.51.100.1' \
  'ROM_UPLOADER_PASSWORD=configured-test-password'
assert_rejected mismatched-route 'must contain DECK_WIREGUARD_ADDRESS' \
  'DECK_SSH_TARGET=root@192.0.2.50' \
  'DECK_WIREGUARD_ADDRESS=198.51.100.11' \
  'DECK_WIREGUARD_ROUTE=203.0.113.0/24' \
  'DECK_WIREGUARD_HEALTH_ADDRESS=203.0.113.1' \
  'ROM_UPLOADER_PASSWORD=configured-test-password'

echo 'deploy-config-test: OK'
