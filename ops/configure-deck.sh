#!/usr/bin/env bash

# Create the private per-Deck deployment configuration.

set -euo pipefail
export LC_ALL=C

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
config=${1:-$repo_root/deck.conf}

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
[[ $target =~ ^root@[A-Za-z0-9._:-]+$ ]] || {
  echo "Invalid Deck SSH target" >&2
  exit 1
}

read -r -s -p 'ROM uploader password (8-128 bytes): ' uploader_password
printf '\n'
read -r -s -p 'Repeat ROM uploader password: ' confirmation
printf '\n'
if [[ $uploader_password != "$confirmation" ]]; then
  echo "Passwords do not match" >&2
  exit 1
fi
if [[ ${#uploader_password} -lt 8 || ${#uploader_password} -gt 128 ||
      $uploader_password == *$'\r'* || $uploader_password == *$'\n'* ]]; then
  echo "Password must contain 8 through 128 bytes without line breaks" >&2
  exit 1
fi

umask 077
temporary=$(mktemp "${config}.new.XXXXXX")
trap 'rm -f "$temporary"' EXIT INT TERM HUP
{
  printf 'DECK_SSH_TARGET=%s\n' "$target"
  printf 'ROM_UPLOADER_PASSWORD=%s\n' "$uploader_password"
} >"$temporary"
chmod 0600 "$temporary"
mv "$temporary" "$config"
trap - EXIT INT TERM HUP

echo "Wrote private Deck configuration to $config"
echo "Deploy with: $repo_root/ops/deploy.sh --config $config"
