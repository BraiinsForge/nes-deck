#!/usr/bin/env bash

# Safely prepare a fresh Deck's Wi-Fi fallback and userspace WireGuard before
# installing the application payload. The live Wi-Fi configuration is treated
# as an invariant throughout: profiles are copied, but wireless is never
# reloaded or switched.

set -euo pipefail
export LC_ALL=C

usage() {
  cat >&2 <<EOF
Usage: $0 [--config PATH] [--wireguard-config PATH]
          [--register-peer-command PATH | --skip-peer-registration]
          [--wifi-profiles DIRECTORY] [--network-only] [--check]
EOF
  exit 2
}

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
config_library=$script_dir/lib/deck-config.sh
config_home=${RETRO_DECK_CONFIG_HOME:-${XDG_CONFIG_HOME:-$HOME/.config}/retro-deck}
config=$config_home/deck.conf
wireguard_config=$config_home/wireguard/wg0.conf
register_peer_command=$config_home/wireguard/register-peer
skip_peer_registration=0
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
    --wireguard-config)
      [[ $# -ge 2 ]] || usage
      wireguard_config=$2
      shift 2
      ;;
    --register-peer-command)
      [[ $# -ge 2 ]] || usage
      register_peer_command=$2
      skip_peer_registration=0
      shift 2
      ;;
    --skip-peer-registration)
      skip_peer_registration=1
      shift
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

[[ -f $config_library && ! -L $config_library ]] || {
  echo "Deck configuration library is missing or unsafe: $config_library" >&2
  exit 1
}
# shellcheck source=ops/lib/deck-config.sh
source "$config_library"
deck_config_load "$config"
target=$DECK_SSH_TARGET
wireguard_address=$DECK_WIREGUARD_ADDRESS
wireguard_route=$DECK_WIREGUARD_ROUTE
wireguard_health_address=$DECK_WIREGUARD_HEALTH_ADDRESS
recovery_wifi_ssid=$DECK_RECOVERY_WIFI_SSID

private_file_valid() {
  local path=$1
  local mode
  [[ -f $path && ! -L $path ]] || return 1
  mode=$(stat -c %a -- "$path") || return 1
  [[ $mode =~ ^[0-7]{3,4}$ ]] || return 1
  (( (8#$mode & 077) == 0 ))
}

private_file_valid "$wireguard_config" || {
  echo "Private WireGuard client configuration is missing or unsafe: $wireguard_config" >&2
  exit 1
}
grep -Eq '^[[:space:]]*\[Peer\][[:space:]]*$' "$wireguard_config" || {
  echo "WireGuard client configuration has no peer" >&2
  exit 1
}
if grep -Eq '^[[:space:]]*PrivateKey[[:space:]]*=' "$wireguard_config"; then
  echo "WireGuard client configuration must not contain a Deck private key" >&2
  exit 1
fi
allowed_ips=$(awk -F= '
  $1 ~ /^[[:space:]]*AllowedIPs[[:space:]]*$/ {
    value=$2
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
    print value
  }
' "$wireguard_config")
[[ $allowed_ips == "$wireguard_route" ]] || {
  echo "WireGuard client AllowedIPs must equal DECK_WIREGUARD_ROUTE" >&2
  exit 1
}
if [[ $skip_peer_registration -eq 0 ]] &&
   { ! private_file_valid "$register_peer_command" ||
     [[ ! -x $register_peer_command ]]; }; then
  echo "Private WireGuard peer registrar is missing or unsafe: $register_peer_command" >&2
  exit 1
fi

for command in awk find grep install mktemp seq sha256sum sort ssh stat tar tr wc xxd; do
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
profile_rank=$work/profile-rank
: >"$profile_rank"
recovery_hex=
if [[ -n $recovery_wifi_ssid ]]; then
  recovery_hex=$(printf '%s' "$recovery_wifi_ssid" | xxd -p | tr -d '\n' |
    tr 'A-F' 'a-f')
fi
shopt -s nullglob
for profile in "$wifi_profiles"/*.psk; do
  [[ -f $profile && ! -L $profile ]] || {
    echo "Refusing unsafe Wi-Fi profile: $profile" >&2
    exit 1
  }
  install -m 0600 -- "$profile" "$profile_stage/${profile##*/}"
  profile_name=${profile##*/}
  profile_name=${profile_name%.psk}
  case $profile_name in
    =*) profile_hex=$(tr 'A-F' 'a-f' <<<"${profile_name#=}") ;;
    *) profile_hex=$(printf '%s' "$profile_name" | xxd -p | tr -d '\n' |
         tr 'A-F' 'a-f') ;;
  esac
  if [[ -z $profile_hex || $profile_hex == *[!0-9a-f]* ||
        $(( ${#profile_hex} % 2 )) -ne 0 || ${#profile_hex} -gt 64 ]]; then
    echo "Refusing malformed Wi-Fi profile filename: ${profile##*/}" >&2
    exit 1
  fi
  autoconnect=$(awk -F= '$1 == "AutoConnect" {value=$2} END {print value}' \
    "$profile")
  if [[ $autoconnect != false ]]; then
    printf '%020d\t%s\n' "$(stat -c %Y -- "$profile")" "$profile_hex" \
      >>"$profile_rank"
  fi
  profile_count=$((profile_count + 1))
done
shopt -u nullglob
[[ $profile_count -gt 0 ]] || {
  echo "No personal PSK profiles found in $wifi_profiles" >&2
  exit 1
}

preferred_stage=$payload/wifi/preferred
sort -rn -k1,1 "$profile_rank" |
  awk -F '\t' -v recovery="$recovery_hex" '
    recovery != "" && $2 == recovery { recovery_present=1; next }
    count < 7 && !seen[$2]++ { print $2; count++ }
    END { if (recovery_present) print recovery }
  ' >"$preferred_stage"
preferred_count=$(wc -l <"$preferred_stage")
[[ $preferred_count -gt 0 ]] || {
  echo "No enabled personal PSK profiles are available for preference seeding" >&2
  exit 1
}
chmod 0600 "$preferred_stage"

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
install -m 0600 "$wireguard_config" "$payload/wireguard/wg0.conf"
install -m 0600 "$wireguard_dir/30-tun" "$payload/wireguard/30-tun"
printf '%s/32\n' "$wireguard_address" >"$payload/wireguard/wg0.address"
printf '%s\n' "$wireguard_route" >"$payload/wireguard/wg0.route"
chmod 0600 "$payload/wireguard/wg0.address"
chmod 0600 "$payload/wireguard/wg0.route"

install -m 0700 "$repo_root/ops/deck-wifi/deck-wifi-profile-add" \
  "$payload/wifi/deck-wifi-profile-add"
install -m 0700 "$repo_root/ops/deck-wifi/deck-wifi-select" \
  "$payload/wifi/deck-wifi-select"
install -m 0700 "$repo_root/ops/deck-wifi/deck-wifi-watch" \
  "$payload/wifi/deck-wifi-watch"
install -m 0700 "$repo_root/ops/deck-wifi/deck-wifi.init" \
  "$payload/wifi/deck-wifi.init"
cp -p "$profile_stage/"*.psk "$payload/wifi/profiles/"

echo "Provision plan: $target -> $wireguard_address over $wireguard_route"
if [[ $skip_peer_registration -eq 1 ]]; then
  echo "WireGuard peer registration: preconfigured externally"
else
  echo "WireGuard peer registration: $register_peer_command"
fi
echo "Wi-Fi intake: $profile_count personal PSK profiles; $ignored_count open/enterprise profiles ignored"
if [[ -n $recovery_wifi_ssid ]]; then
  echo "Wi-Fi preference seed: $preferred_count profiles; configured recovery profile last when present"
else
  echo "Wi-Fi preference seed: $preferred_count profiles in recency order"
fi
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
  "$remote_stage" "$wireguard_address/32" "$wireguard_route" \
  >"$work/deck-public-key" <<'DECK'
set -eu
stage=$1
wireguard_address=$2
wireguard_route=$3
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
if [ -s /etc/wireguard/wg0.route ]; then
  [ "$(cat /etc/wireguard/wg0.route)" = "$wireguard_route" ] || {
    echo "Refusing to change an existing Deck WireGuard route" >&2
    exit 1
  }
else
  cp -p "$stage/wireguard/wg0.route" /etc/wireguard/wg0.route
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
  /etc/wireguard/wg0.address /etc/wireguard/wg0.route

cp -p "$stage/wifi/deck-wifi-profile-add" \
  /usr/sbin/deck-wifi-profile-add
cp -p "$stage/wifi/deck-wifi-select" /usr/sbin/deck-wifi-select
cp -p "$stage/wifi/deck-wifi-watch" /usr/sbin/deck-wifi-watch
cp -p "$stage/wifi/deck-wifi.init" /etc/init.d/deck-wifi
cp -p "$stage/wifi/profiles/"*.psk /etc/deck-wifi/profiles/
if [ ! -s /etc/deck-wifi/preferred ]; then
  cp -p "$stage/wifi/preferred" /etc/deck-wifi/preferred
fi
chmod 0700 /usr/sbin/deck-wifi-profile-add \
  /usr/sbin/deck-wifi-select /usr/sbin/deck-wifi-watch \
  /etc/init.d/deck-wifi
chmod 0600 /etc/deck-wifi/profiles/*.psk
chmod 0600 /etc/deck-wifi/preferred
/mnt/data/nes-deck/wireguard/bin/wg pubkey \
  </etc/wireguard/wg0.key
DECK

deck_public_key=$(tail -n 1 "$work/deck-public-key")
[[ $deck_public_key =~ ^[A-Za-z0-9+/]{43}=$ ]] || {
  echo "Deck returned an invalid WireGuard public key" >&2
  exit 1
}

if [[ $skip_peer_registration -eq 0 ]]; then
  echo "Registering the peer through the private external command..."
  "$register_peer_command" "$wireguard_address/32" "$deck_public_key"
else
  echo "Skipping peer registration because the server was preconfigured externally."
fi

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
    "ping -c 1 -W 2 '$wireguard_health_address' >/dev/null 2>&1 &&
     ip route get '$wireguard_health_address' |
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
  /etc/init.d/bmc-compositor status >/dev/null
  /etc/init.d/nes-deck-uploader status >/dev/null
  test -s /run/current-profile/lib/bmc-widgets/retro-deck/manifest.json
  test -x /run/current-profile/lib/bmc-widgets/retro-deck/bin/retro-deck
  test -s /run/current-profile/lib/bmc-applications/retro-deck/manifest.json
  test -x /run/current-profile/lib/bmc-applications/retro-deck/bin/retro-deck-launcher
  grep -q '73219c9d-f1ef-41dc-960c-d0711e42a6ac' /etc/bmc_config.json
  pidof rom-uploader >/dev/null
  ip route get '$wireguard_health_address' |
    grep -q 'dev wg0.*src $wireguard_address'
"
echo "Fresh Deck provisioning and application deployment complete."
