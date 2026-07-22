#!/usr/bin/env bash

set -euo pipefail

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
fixture=$(mktemp -d /tmp/check-deck-test.XXXXXX)
trap 'rm -rf "$fixture"' EXIT INT TERM HUP
config=$fixture/deck.conf
arguments=$fixture/ssh-arguments
timeout_arguments=$fixture/timeout-arguments
remote_script=$fixture/remote-script
mkdir -p "$fixture/bin"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

cat >"$fixture/bin/ssh" <<'MOCK'
#!/bin/sh
printf '%s\n' "$*" >"$CHECK_DECK_ARGUMENTS"
cat >"$CHECK_DECK_REMOTE_SCRIPT"
[ "${CHECK_DECK_SSH_FAIL:-0}" -eq 0 ] || exit 255
printf '%s\n' 'RESULT         HEALTHY'
MOCK
chmod 0700 "$fixture/bin/ssh"

cat >"$fixture/bin/timeout" <<'MOCK'
#!/bin/sh
printf '%s\n' "$*" >"$CHECK_DECK_TIMEOUT_ARGUMENTS"
shift
exec "$@"
MOCK
chmod 0700 "$fixture/bin/timeout"

cat >"$config" <<'CONFIG'
DECK_SSH_TARGET=root@192.168.1.50
DECK_WIREGUARD_ADDRESS=10.0.0.11
ROM_UPLOADER_PASSWORD=configured-test-password
CONFIG
chmod 0600 "$config"

export CHECK_DECK_ARGUMENTS=$arguments
export CHECK_DECK_TIMEOUT_ARGUMENTS=$timeout_arguments
export CHECK_DECK_REMOTE_SCRIPT=$remote_script
output=$(PATH="$fixture/bin:$PATH" \
  "$repo_root/ops/check-deck.sh" --config "$config")
grep -Fq 'Checking Retro Deck at root@192.168.1.50...' <<<"$output" ||
  fail 'health check did not identify its target'
grep -Fq -- '-o BatchMode=yes -o ConnectTimeout=7 -o LogLevel=ERROR root@192.168.1.50 sh -s' \
  "$arguments" || fail 'health check did not use bounded batch SSH'
grep -Fq '20 ssh -o BatchMode=yes' "$timeout_arguments" ||
  fail 'health check did not enforce its overall SSH timeout'
grep -Fq "pidof \"\$process\"" "$remote_script" ||
  fail 'remote health check does not inspect required processes'
grep -Fq "grep -q ' /mnt/data ' /proc/mounts" "$remote_script" ||
  fail 'remote health check does not verify persistent storage'
grep -Fq '[ -s /mnt/data/nes-deck/lisp/ui.lisp ]' "$remote_script" ||
  fail 'remote health check does not verify the editable Lisp UI'
grep -Fq '[ -s /mnt/data/nes-deck/lisp/policy.lisp ]' "$remote_script" ||
  fail 'remote health check does not verify the editable Lisp policy'
grep -Fq '[ -s /mnt/data/nes-deck/lisp/credits.lisp ]' "$remote_script" ||
  fail 'remote health check does not verify the editable Lisp credits'
grep -Fq '[ -s /mnt/data/nes-deck/lisp/dashboard.lisp ]' "$remote_script" ||
  fail 'remote health check does not verify the Lisp dashboard'
grep -Fq 'interface_address wlan0' "$remote_script" ||
  fail 'remote health check does not report the WLAN address'
grep -Fq '/usr/sbin/iw dev wlan0 link' "$remote_script" ||
  fail 'remote health check does not report the associated SSID'
grep -Fq "tr -cd ' -~'" "$remote_script" ||
  fail 'remote health check does not use BusyBox-compatible SSID sanitizing'
grep -Fq "tail -n 20 \"\$log\"" "$remote_script" ||
  fail 'remote health check does not include the bounded dashboard log'

PATH="$fixture/bin:$PATH" "$repo_root/ops/check-deck.sh" \
  --config "$config" root@10.0.1.7 >/dev/null
grep -Fq 'root@10.0.1.7 sh -s' "$arguments" ||
  fail 'health check did not honor the temporary SSH target override'

if CHECK_DECK_SSH_FAIL=1 PATH="$fixture/bin:$PATH" \
  "$repo_root/ops/check-deck.sh" --config "$config" \
  >"$fixture/failure-output" 2>"$fixture/failure-error"; then
  fail 'health check succeeded after SSH failed'
fi
grep -Fq 'health check failed or timed out' "$fixture/failure-error" ||
  fail 'health check did not explain the failed SSH operation'

printf 'check-deck-test: OK\n'
