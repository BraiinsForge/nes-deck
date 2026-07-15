#!/usr/bin/env bash

# Safely prepare a fresh Deck's Wi-Fi fallback and userspace WireGuard before
# installing the application payload. The live Wi-Fi configuration is treated
# as an invariant throughout: profiles are copied, but wireless is never
# reloaded or switched.

set -euo pipefail
export LC_ALL=C

usage() {
  cat >&2 <<EOF
Usage: $0 [--config PATH] [--wireguard-server root@HOST]
          [--wifi-profiles DIRECTORY] [--network-only] [--check]
EOF
  exit 2
}

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
config=$repo_root/deck.conf
wireguard_server=root@10.0.0.1
wifi_profiles=/var/lib/iwd
network_only=0
check_only=0

while [[ $# -gt 0 ]]; do
  case $1 in
    --config)
      [[ $# -ge 2 ]] || usage
      config=$2
      shift 2
      ;;
    --wireguard-server)
      [[ $# -ge 2 ]] || usage
      wireguard_server=$2
      shift 2
      ;;
    --wifi-profiles)
      [[ $# -ge 2 ]] || usage
      wifi_profiles=$2
      shift 2
      ;;
    --network-only)
      network_only=1
      shift
      ;;
    --check)
      check_only=1
      shift
      ;;
    *)
      usage
      ;;
  esac
done

[[ $wireguard_server =~ ^root@[A-Za-z0-9._:-]+$ ]] || {
  echo "WireGuard server must have the form root@HOST" >&2
  exit 1
}

"$repo_root/ops/deploy.sh" --config "$config" --check-config >/dev/null
target=$(awk -F= '$1 == "DECK_SSH_TARGET" { print substr($0, index($0, "=") + 1) }' "$config")
wireguard_address=$(awk -F= '$1 == "DECK_WIREGUARD_ADDRESS" { print substr($0, index($0, "=") + 1) }' "$config")

for command in awk find install mktemp sha256sum ssh tar; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done

wireguard_dir=$repo_root/ops/deck-wireguard
(cd "$wireguard_dir" && sha256sum -c SHA256SUMS >/dev/null)

[[ -d $wifi_profiles && ! -L $wifi_profiles ]] || {
  echo "Wi-Fi profile directory is missing or unsafe: $wifi_profiles" >&2
  exit 1
}

work=$(mktemp -d "${TMPDIR:-/tmp}/nes-deck-provision.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM HUP
profile_stage=$work/profiles
payload=$work/payload
mkdir -p "$profile_stage" \
  "$payload/wireguard/bin" "$payload/wifi/profiles"

profile_count=0
shopt -s nullglob
for profile in "$wifi_profiles"/*.psk; do
  [[ -f $profile && ! -L $profile ]] || {
    echo "Refusing unsafe Wi-Fi profile: $profile" >&2
    exit 1
  }
  install -m 0600 -- "$profile" "$profile_stage/${profile##*/}"
  profile_count=$((profile_count + 1))
done
shopt -u nullglob
[[ $profile_count -gt 0 ]] || {
  echo "No personal PSK profiles found in $wifi_profiles" >&2
  exit 1
}

ignored_count=$(find "$wifi_profiles" -maxdepth 1 -type f \
  \( -name '*.8021x' -o -name '*.open' \) -print | wc -l)

install -m 0700 "$wireguard_dir/bin/wireguard-go" \
  "$payload/wireguard/bin/wireguard-go"
install -m 0700 "$wireguard_dir/bin/wg" "$payload/wireguard/bin/wg"
install -m 0600 "$wireguard_dir/bin/tun.ko" "$payload/wireguard/tun.ko"
install -m 0700 "$wireguard_dir/deck-wireguard-run" \
  "$payload/wireguard/deck-wireguard-run"
install -m 0700 "$wireguard_dir/deck-wireguard.init" \
  "$payload/wireguard/deck-wireguard.init"
install -m 0600 "$wireguard_dir/wg0.conf" "$payload/wireguard/wg0.conf"
install -m 0600 "$wireguard_dir/30-tun" "$payload/wireguard/30-tun"
printf '%s/32\n' "$wireguard_address" >"$payload/wireguard/wg0.address"
chmod 0600 "$payload/wireguard/wg0.address"

install -m 0700 "$repo_root/ops/deck-wifi/deck-wifi-profile-add" \
  "$payload/wifi/deck-wifi-profile-add"
install -m 0700 "$repo_root/ops/deck-wifi/deck-wifi-select" \
  "$payload/wifi/deck-wifi-select"
install -m 0700 "$repo_root/ops/deck-wifi/deck-wifi-watch" \
  "$payload/wifi/deck-wifi-watch"
install -m 0700 "$repo_root/ops/deck-wifi/deck-wifi.init" \
  "$payload/wifi/deck-wifi.init"
cp -p "$profile_stage/"*.psk "$payload/wifi/profiles/"

echo "Provision plan: $target -> $wireguard_address via $wireguard_server"
echo "Wi-Fi intake: $profile_count personal PSK profiles; $ignored_count open/enterprise profiles ignored"
if [[ $check_only -eq 1 ]]; then
  echo "Provision inputs are valid; no remote state was changed."
  exit 0
fi

echo "Checking the live Deck network before installation..."
readarray -t network_before < <(ssh -o BatchMode=yes "$target" '
  set -eu
  grep -q "[[:space:]]/mnt/data[[:space:]]" /proc/mounts
  [ "$(uname -r)" = 5.10.176 ]
  [ -f /etc/config/wireless ] && [ ! -L /etc/config/wireless ]
  sha256sum /etc/config/wireless | awk "{print \$1}"
  ip -4 -o address show dev wlan0 | awk "NR == 1 {print \$4}"
  ip -4 route show default | awk "NR == 1 {print \$0}"
')
[[ ${#network_before[@]} -eq 3 && -n ${network_before[0]} &&
   -n ${network_before[1]} && ${network_before[2]} == *" dev wlan0 "* ]] || {
  echo "Deck has no safe live Wi-Fi address/default-route snapshot" >&2
  exit 1
}

remote_stage=/mnt/data/.nes-deck-provision-$$
echo "Installing network helpers without reloading Wi-Fi..."
tar -C "$payload" -czf - . |
  ssh -o BatchMode=yes "$target" \
    "umask 077; cat >'$remote_stage.tar.gz'"

ssh -o BatchMode=yes "$target" sh -s -- \
  "$remote_stage" "$wireguard_address/32" \
  >"$work/deck-public-key" <<'DECK'
set -eu
stage=$1
wireguard_address=$2
archive=$stage.tar.gz
trap 'rm -rf "$stage" "$archive"' EXIT INT TERM HUP
rm -rf "$stage"
mkdir -p "$stage"
tar -xzf "$archive" -C "$stage"

mkdir -p /mnt/data/nes-deck/wireguard/bin /etc/wireguard \
  /etc/deck-wifi/profiles /etc/deck-wifi/backups /dev/net
chmod 0700 /etc/wireguard /etc/deck-wifi \
  /etc/deck-wifi/profiles /etc/deck-wifi/backups

cp -p "$stage/wireguard/bin/wireguard-go" \
  /mnt/data/nes-deck/wireguard/bin/wireguard-go
cp -p "$stage/wireguard/bin/wg" \
  /mnt/data/nes-deck/wireguard/bin/wg
cp -p "$stage/wireguard/deck-wireguard-run" \
  /mnt/data/nes-deck/wireguard/deck-wireguard-run
cp -p "$stage/wireguard/tun.ko" /lib/modules/5.10.176/tun.ko
cp -p "$stage/wireguard/30-tun" /etc/modules.d/30-tun
cp -p "$stage/wireguard/wg0.conf" /etc/wireguard/wg0.conf
cp -p "$stage/wireguard/deck-wireguard.init" \
  /etc/init.d/deck-wireguard

if [ -s /etc/wireguard/wg0.address ]; then
  [ "$(cat /etc/wireguard/wg0.address)" = "$wireguard_address" ] || {
    echo "Refusing to change an existing Deck WireGuard address" >&2
    exit 1
  }
else
  cp -p "$stage/wireguard/wg0.address" /etc/wireguard/wg0.address
fi
if [ ! -s /etc/wireguard/wg0.key ]; then
  key=/etc/wireguard/.wg0.key.new.$$
  /mnt/data/nes-deck/wireguard/bin/wg genkey >"$key"
  chmod 0600 "$key"
  mv "$key" /etc/wireguard/wg0.key
fi
chmod 0700 /mnt/data/nes-deck/wireguard/bin/wireguard-go \
  /mnt/data/nes-deck/wireguard/bin/wg \
  /mnt/data/nes-deck/wireguard/deck-wireguard-run \
  /etc/init.d/deck-wireguard
chmod 0600 /lib/modules/5.10.176/tun.ko /etc/modules.d/30-tun \
  /etc/wireguard/wg0.conf /etc/wireguard/wg0.key \
  /etc/wireguard/wg0.address

cp -p "$stage/wifi/deck-wifi-profile-add" \
  /usr/sbin/deck-wifi-profile-add
cp -p "$stage/wifi/deck-wifi-select" /usr/sbin/deck-wifi-select
cp -p "$stage/wifi/deck-wifi-watch" /usr/sbin/deck-wifi-watch
cp -p "$stage/wifi/deck-wifi.init" /etc/init.d/deck-wifi
cp -p "$stage/wifi/profiles/"*.psk /etc/deck-wifi/profiles/
chmod 0700 /usr/sbin/deck-wifi-profile-add \
  /usr/sbin/deck-wifi-select /usr/sbin/deck-wifi-watch \
  /etc/init.d/deck-wifi
chmod 0600 /etc/deck-wifi/profiles/*.psk
/mnt/data/nes-deck/wireguard/bin/wg pubkey \
  </etc/wireguard/wg0.key
DECK

deck_public_key=$(tail -n 1 "$work/deck-public-key")
[[ $deck_public_key =~ ^[A-Za-z0-9+/]{43}=$ ]] || {
  echo "Deck returned an invalid WireGuard public key" >&2
  exit 1
}

echo "Reserving the unique peer on the WireGuard server..."
ssh -o BatchMode=yes "$wireguard_server" sh -s -- \
  "$wireguard_address/32" "$deck_public_key" <<'SERVER'
set -eu
address=$1
public_key=$2
config=/etc/wireguard/wg0.conf

[ -f "$config" ] && [ ! -L "$config" ] || {
  echo "Unsafe or missing server WireGuard configuration" >&2
  exit 1
}
wg show wg0 >/dev/null

persistent_key=$(awk -v wanted="$address" '
  function emit() {
    if (allowed == wanted && key != "") print key
  }
  $1 == "[Peer]" { emit(); key=""; allowed=""; next }
  $1 == "PublicKey" && $2 == "=" { key=$3; next }
  $1 == "AllowedIPs" && $2 == "=" { allowed=$3; next }
  END { emit() }
' "$config")
case $persistent_key in
  "") ;;
  "$public_key") ;;
  *)
    echo "WireGuard address is already assigned to another key: $address" >&2
    exit 1
    ;;
esac

key_address=$(awk -v wanted="$public_key" '
  function emit() {
    if (key == wanted && allowed != "") print allowed
  }
  $1 == "[Peer]" { emit(); key=""; allowed=""; next }
  $1 == "PublicKey" && $2 == "=" { key=$3; next }
  $1 == "AllowedIPs" && $2 == "=" { allowed=$3; next }
  END { emit() }
' "$config")
case $key_address in
  "") ;;
  "$address") ;;
  *)
    echo "Deck public key is already assigned to another address: $key_address" >&2
    exit 1
    ;;
esac

live_key=$(wg show wg0 allowed-ips |
  awk -v wanted="$address" '$2 == wanted { print $1 }')
case $live_key in
  "") ;;
  "$public_key") ;;
  *)
    echo "Live WireGuard address is already assigned: $address" >&2
    exit 1
    ;;
esac

if [ -z "$persistent_key" ]; then
  backup_dir=/etc/wireguard/backups
  mkdir -p "$backup_dir"
  chmod 0700 "$backup_dir"
  timestamp=$(date -u '+%Y%m%dT%H%M%SZ')
  cp -p "$config" "$backup_dir/wg0.conf.$timestamp"
  temporary=$(mktemp /etc/wireguard/.wg0.conf.new.XXXXXX)
  trap 'rm -f "$temporary"' EXIT INT TERM HUP
  {
    cat "$config"
    printf '\n[Peer]\nPublicKey = %s\nAllowedIPs = %s\n' \
      "$public_key" "$address"
  } >"$temporary"
  chmod 0600 "$temporary"
  chown 0:0 "$temporary"
  mv "$temporary" "$config"
  trap - EXIT INT TERM HUP
fi

if [ -z "$live_key" ]; then
  wg set wg0 peer "$public_key" allowed-ips "$address"
fi

[ "$(wg show wg0 allowed-ips | awk -v wanted="$address" \
  '$2 == wanted { print $1 }')" = "$public_key" ]
SERVER

echo "Starting the guarded Wi-Fi watcher and WireGuard tunnel..."
ssh -o BatchMode=yes "$target" '
  set -eu
  /etc/init.d/deck-wifi enable
  /etc/init.d/deck-wifi status >/dev/null 2>&1 ||
    /etc/init.d/deck-wifi start
  /etc/init.d/deck-wireguard enable
  /etc/init.d/deck-wireguard status >/dev/null 2>&1 ||
    /etc/init.d/deck-wireguard start
'

for attempt in $(seq 1 45); do
  if ssh -o BatchMode=yes -o ConnectTimeout=4 "$target" \
    "ping -c 1 -W 2 10.0.0.1 >/dev/null 2>&1 &&
     ip route get 10.0.0.1 |
       grep -q 'dev wg0.*src $wireguard_address'" \
    >/dev/null 2>&1; then
    break
  fi
  [[ $attempt -lt 45 ]] || {
    echo "Deck WireGuard tunnel did not become ready" >&2
    exit 1
  }
  sleep 2
done

readarray -t network_after < <(ssh -o BatchMode=yes "$target" '
  set -eu
  sha256sum /etc/config/wireless | awk "{print \$1}"
  ip -4 -o address show dev wlan0 | awk "NR == 1 {print \$4}"
  ip -4 route show default | awk "NR == 1 {print \$0}"
')
[[ ${#network_after[@]} -eq 3 &&
   ${network_after[0]} == "${network_before[0]}" &&
   ${network_after[1]} == "${network_before[1]}" &&
   ${network_after[2]} == "${network_before[2]}" ]] || {
  echo "Live Wi-Fi changed during network provisioning; stopping before application deployment" >&2
  exit 1
}

echo "Network provisioning passed without changing live Wi-Fi."
if [[ $network_only -eq 1 ]]; then
  echo "Network-only provisioning complete."
  exit 0
fi

"$repo_root/ops/deploy.sh" --config "$config"

ssh -o BatchMode=yes "$target" "
  set -eu
  /etc/init.d/deck-wifi status >/dev/null
  /etc/init.d/deck-wireguard status >/dev/null
  /etc/init.d/nes-deck status >/dev/null
  /etc/init.d/nes-deck-uploader status >/dev/null
  pidof deck-menu >/dev/null
  pidof rom-uploader >/dev/null
  ip route get 10.0.0.1 | grep -q 'dev wg0.*src $wireguard_address'
"
echo "Fresh Deck provisioning and application deployment complete."
