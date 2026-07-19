#!/usr/bin/env bash

set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
fixture=$(mktemp -d /tmp/nes-deck-provision-test.XXXXXX)
trap 'rm -rf "$fixture"' EXIT INT TERM HUP
config=$fixture/deck.conf
profiles=$fixture/iwd
wireguard_config=$fixture/wg0.conf
register_peer=$fixture/register-peer
mkdir "$profiles"

cat >"$config" <<'EOF'
DECK_SSH_TARGET=root@192.0.2.60
DECK_WIREGUARD_ADDRESS=198.51.100.13
DECK_WIREGUARD_ROUTE=198.51.100.0/24
DECK_WIREGUARD_HEALTH_ADDRESS=198.51.100.1
ROM_UPLOADER_PASSWORD=configured-test-password
EOF
chmod 0600 "$config"

cat >"$wireguard_config" <<'EOF'
[Interface]

[Peer]
PublicKey = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
AllowedIPs = 198.51.100.0/24
Endpoint = vpn.example.test:51820
PersistentKeepalive = 25
EOF
chmod 0600 "$wireguard_config"

cat >"$register_peer" <<'EOF'
#!/bin/sh
exit 0
EOF
chmod 0700 "$register_peer"

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
  --wireguard-config "$wireguard_config" \
  --register-peer-command "$register_peer" \
  --wifi-profiles "$profiles" --check)
grep -qx 'Provision plan: root@192.0.2.60 -> 198.51.100.13 over 198.51.100.0/24' \
  <<<"$output"
grep -qx "WireGuard peer registration: $register_peer" <<<"$output"
grep -qx 'Wi-Fi intake: 2 personal PSK profiles; 2 open/enterprise profiles ignored' \
  <<<"$output"
grep -qx 'Wi-Fi preference seed: 2 recent profiles; recovery profile last when present' \
  <<<"$output"
grep -qx 'Provision inputs are valid; no remote state was changed.' <<<"$output"

output=$("$root/ops/provision-deck.sh" --config "$config" \
  --wireguard-config "$wireguard_config" \
  --skip-peer-registration \
  --wifi-profiles "$profiles" --check)
grep -qx 'WireGuard peer registration: preconfigured externally' <<<"$output"

ln -s home.psk "$profiles/unsafe.psk"
if "$root/ops/provision-deck.sh" --config "$config" \
  --wireguard-config "$wireguard_config" \
  --register-peer-command "$register_peer" \
  --wifi-profiles "$profiles" --check >/dev/null 2>&1; then
  echo "unsafe profile symlink was accepted" >&2
  exit 1
fi
rm "$profiles/unsafe.psk"

chmod 0600 "$register_peer"
if "$root/ops/provision-deck.sh" --config "$config" \
  --wireguard-config "$wireguard_config" \
  --register-peer-command "$register_peer" \
  --wifi-profiles "$profiles" \
  --check >/dev/null 2>&1; then
  echo "non-executable WireGuard peer registrar was accepted" >&2
  exit 1
fi

echo "provision-config-test: OK"
