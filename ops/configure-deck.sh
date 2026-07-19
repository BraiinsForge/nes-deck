#!/usr/bin/env bash

# Create the private per-Deck deployment configuration.

set -euo pipefail
export LC_ALL=C

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
config_library=$script_dir/lib/deck-config.sh
config_home=${RETRO_DECK_CONFIG_HOME:-${XDG_CONFIG_HOME:-$HOME/.config}/retro-deck}
config=${1:-$config_home/deck.conf}

[[ -f $config_library && ! -L $config_library ]] || {
  echo "Deck configuration library is missing or unsafe: $config_library" >&2
  exit 1
}
# shellcheck source=ops/lib/deck-config.sh
source "$config_library"

if [[ $# -gt 1 ]]; then
  echo "Usage: $0 [CONFIG]" >&2
  exit 2
fi
if [[ -e $config || -L $config ]]; then
  echo "Refusing to replace existing configuration: $config" >&2
  exit 1
fi
if [[ $# -eq 0 && ! -e $config_home ]]; then
  install -d -m 0700 -- "$config_home"
fi
if [[ ! -d $(dirname -- "$config") || -L $(dirname -- "$config") ]]; then
  echo "Configuration directory does not exist: $(dirname -- "$config")" >&2
  exit 1
fi

read -r -p 'Deck SSH target (root@IP): ' target
deck_config_valid_ssh_target "$target" || {
  echo "Deck SSH target must have the form root@IP" >&2
  exit 1
}

read -r -p 'Deck WireGuard IPv4 address: ' wireguard_address
if ! deck_config_valid_wireguard_address "$wireguard_address"; then
  echo "Deck WireGuard address must be canonical unicast IPv4" >&2
  exit 1
fi

read -r -p 'WireGuard routed IPv4 prefix: ' wireguard_route
if ! deck_config_valid_wireguard_route "$wireguard_route"; then
  echo "WireGuard route must be a canonical IPv4 network prefix" >&2
  exit 1
fi
if ! deck_config_route_contains_address "$wireguard_route" "$wireguard_address"; then
  echo "WireGuard route must contain the Deck address" >&2
  exit 1
fi

read -r -p 'WireGuard health-check IPv4 address: ' wireguard_health_address
if ! deck_config_valid_wireguard_address "$wireguard_health_address"; then
  echo "WireGuard health address must be canonical unicast IPv4" >&2
  exit 1
fi
if ! deck_config_route_contains_address \
  "$wireguard_route" "$wireguard_health_address"; then
  echo "WireGuard route must contain the health address" >&2
  exit 1
fi
if [[ $wireguard_health_address == "$wireguard_address" ]]; then
  echo "WireGuard health address must differ from the Deck address" >&2
  exit 1
fi

read -r -p 'Recovery Wi-Fi SSID (blank for none): ' recovery_wifi_ssid
if ! deck_config_valid_recovery_wifi_ssid "$recovery_wifi_ssid"; then
  echo "Recovery Wi-Fi SSID must contain at most 32 bytes without line breaks" >&2
  exit 1
fi

read -r -s -p 'ROM uploader password (8-128 bytes): ' uploader_password
printf '\n'
read -r -s -p 'Repeat ROM uploader password: ' confirmation
printf '\n'
if [[ $uploader_password != "$confirmation" ]]; then
  echo "Passwords do not match" >&2
  exit 1
fi
if ! deck_config_valid_uploader_password "$uploader_password"; then
  echo "ROM uploader password must contain 8 through 128 bytes without line breaks" >&2
  exit 1
fi

umask 077
temporary=$(mktemp "${config}.new.XXXXXX")
trap 'rm -f "$temporary"' EXIT INT TERM HUP
{
  printf 'DECK_SSH_TARGET=%s\n' "$target"
  printf 'DECK_WIREGUARD_ADDRESS=%s\n' "$wireguard_address"
  printf 'DECK_WIREGUARD_ROUTE=%s\n' "$wireguard_route"
  printf 'DECK_WIREGUARD_HEALTH_ADDRESS=%s\n' "$wireguard_health_address"
  printf 'DECK_RECOVERY_WIFI_SSID=%s\n' "$recovery_wifi_ssid"
  printf 'ROM_UPLOADER_PASSWORD=%s\n' "$uploader_password"
} >"$temporary"
chmod 0600 "$temporary"
mv "$temporary" "$config"
trap - EXIT INT TERM HUP

echo "Wrote private Deck configuration to $config"
echo "Provision a fresh Deck with: $repo_root/ops/provision-deck.sh --config $config"
echo "Update an existing Deck with: $repo_root/ops/deploy.sh --config $config"
