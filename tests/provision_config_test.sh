#!/usr/bin/env bash

set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
fixture=$(mktemp -d /tmp/nes-deck-provision-test.XXXXXX)
trap 'rm -rf "$fixture"' EXIT INT TERM HUP
config=$fixture/deck.conf
profiles=$fixture/iwd
mkdir "$profiles"

cat >"$config" <<'EOF'
DECK_SSH_TARGET=root@192.168.1.60
DECK_WIREGUARD_ADDRESS=10.0.0.13
ROM_UPLOADER_PASSWORD=configured-test-password
EOF
chmod 0600 "$config"

cat >"$profiles/home.psk" <<'EOF'
[Security]
PreSharedKey=fixture
EOF
cat >"$profiles/BraiinsRecovery.psk" <<'EOF'
[Security]
Passphrase=12345678
EOF
cat >"$profiles/cafe.open" <<'EOF'
[Settings]
AutoConnect=true
EOF
cat >"$profiles/enterprise.8021x" <<'EOF'
[Security]
EAP-Method=PEAP
EOF

output=$("$root/ops/provision-deck.sh" --config "$config" \
  --wifi-profiles "$profiles" --check)
grep -qx 'Provision plan: root@192.168.1.60 -> 10.0.0.13 via root@10.0.0.1' \
  <<<"$output"
grep -qx 'Wi-Fi intake: 2 personal PSK profiles; 2 open/enterprise profiles ignored' \
  <<<"$output"
grep -qx 'Wi-Fi preference seed: 2 recent profiles; recovery profile last when present' \
  <<<"$output"
grep -qx 'Provision inputs are valid; no remote state was changed.' <<<"$output"

ln -s home.psk "$profiles/unsafe.psk"
if "$root/ops/provision-deck.sh" --config "$config" \
  --wifi-profiles "$profiles" --check >/dev/null 2>&1; then
  echo "unsafe profile symlink was accepted" >&2
  exit 1
fi
rm "$profiles/unsafe.psk"

if "$root/ops/provision-deck.sh" --config "$config" \
  --wifi-profiles "$profiles" --wireguard-server nobody@example.test \
  --check >/dev/null 2>&1; then
  echo "non-root WireGuard server was accepted" >&2
  exit 1
fi

echo "provision-config-test: OK"
