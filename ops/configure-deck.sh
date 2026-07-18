#!/usr/bin/env bash

# Create the private per-Deck deployment configuration.

set -euo pipefail
export LC_ALL=C

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
config_library=$script_dir/lib/deck-config.sh
config=${1:-$repo_root/deck.conf}

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
if [[ ! -d $(dirname -- "$config") ]]; then
  echo "Configuration directory does not exist: $(dirname -- "$config")" >&2
  exit 1
fi

read -r -p 'Deck SSH target (root@IP): ' target
deck_config_valid_ssh_target "$target" || {
  echo "Deck SSH target must have the form root@IP" >&2
  exit 1
}

read -r -p 'Deck WireGuard address (10.0.0.2-253): ' wireguard_address
if ! deck_config_valid_wireguard_address "$wireguard_address"; then
  echo "Deck WireGuard address must be between 10.0.0.2 and 10.0.0.253" >&2
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
  printf 'ROM_UPLOADER_PASSWORD=%s\n' "$uploader_password"
} >"$temporary"
chmod 0600 "$temporary"
mv "$temporary" "$config"
trap - EXIT INT TERM HUP

echo "Wrote private Deck configuration to $config"
echo "Provision a fresh Deck with: $repo_root/ops/provision-deck.sh --config $config"
echo "Update an existing Deck with: $repo_root/ops/deploy.sh --config $config"
