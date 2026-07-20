#!/usr/bin/env bash

# Build and inspect every deployable ARM runtime.

set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
cd "$repo_root"

for command in file nix nix-store; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done

build_flake() {
  local attribute=$1
  nix build --no-link --print-out-paths "$attribute" | tail -n 1
}

verify_closure_free() {
  local package=$1
  local output=$2
  local references
  references=$(nix-store -q --references "$output")
  if [[ -n $references ]]; then
    echo "$package retains forbidden Nix store references:" >&2
    printf '%s\n' "$references" >&2
    return 1
  fi
}

verify_arm_executable() {
  local package=$1
  local output=$2
  local relative_path=$3
  local executable=$output/$relative_path
  [[ -x $executable ]] || {
    echo "$package is missing executable $relative_path" >&2
    return 1
  }
  local description
  description=$(file -b "$executable")
  case $description in
    *"ELF 32-bit LSB executable, ARM"*"statically linked"*) ;;
    *)
      echo "$package produced an unexpected executable: $description" >&2
      return 1
      ;;
  esac
}

verify_package() {
  local attribute=$1
  shift
  local output
  output=$(build_flake ".#$attribute")
  verify_closure_free "$attribute" "$output"
  for relative_path in "$@"; do
    verify_arm_executable "$attribute" "$output" "$relative_path"
  done
  echo "$attribute: OK"
}

verify_package nes-deck bin/nes-deck
verify_package gb-deck bin/gb-deck
verify_package zx-deck bin/zx-deck
verify_package chip8-deck bin/chip8-deck
verify_package ten-seconds-deck bin/ten-seconds-deck
verify_package deck-dashboard bin/deck-dashboard
verify_package deck-menu bin/deck-menu
verify_package fbterm-deck bin/fbterm bin/loadkeys
verify_package rlwrap-deck bin/rlwrap
verify_package lua-deck bin/lua
verify_package python-deck bin/python
verify_package chibi-deck bin/chibi-scheme
verify_package chiptune-deck bin/chiptune-deck
verify_package rom-uploader bin/rom-uploader

runtime_licenses=$(build_flake .#runtime-licenses)
verify_closure_free runtime-licenses "$runtime_licenses"
[[ -s $runtime_licenses/share/licenses/runtime/Wayland-COPYING ]] || {
  echo "runtime-licenses is missing the Wayland notice" >&2
  exit 1
}
echo "runtime-licenses: OK"

ecl=$(nix build --no-link --print-out-paths -f nix/ecl-arm-static.nix |
  tail -n 1)
verify_closure_free ecl-arm-static "$ecl"
verify_arm_executable ecl-arm-static "$ecl" bin/ecl.bin
[[ -s $ecl/lib/ecl/help.doc ]] || {
  echo "ecl-arm-static is missing lib/ecl/help.doc" >&2
  exit 1
}
[[ -s $ecl/share/licenses/ecl-deck/ECL-LICENSE ]] || {
  echo "ecl-arm-static is missing its license archive" >&2
  exit 1
}
echo "ecl-arm-static: OK"

fbterm=$(build_flake .#fbterm-deck)
[[ -s $fbterm/share/retro-deck/keymaps/us.map ]]
[[ -s $fbterm/share/retro-deck/keymaps/cz.map ]]

(cd chiptunes && sha256sum -c SHA256SUMS)

echo "All ARM payloads passed."
