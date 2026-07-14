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

printf '%s\n%s\n%s\n' \
  'root@192.168.1.50' "$password" "$password" |
  "$root/ops/configure-deck.sh" "$config" >/dev/null

[[ $(stat -c %a "$config") == 600 ]] || fail 'configuration is private'
grep -qx 'DECK_SSH_TARGET=root@192.168.1.50' "$config" ||
  fail 'SSH target was not stored'
grep -qx "ROM_UPLOADER_PASSWORD=$password" "$config" ||
  fail 'uploader password was not stored'
"$root/ops/deploy.sh" --config "$config" --check-config |
  grep -qx 'Deck configuration is valid for root@192.168.1.50' ||
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

echo 'deploy-config-test: OK'
