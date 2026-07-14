#!/usr/bin/env bash

# Build and run every host-side regression test without polluting the repo.

set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
cd "$repo_root"

cxx=${CXX:-g++}
cc=${CC:-cc}
for command in "$cxx" "$cc" nix pkg-config; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done
pkg-config --exists libpng || {
  echo "Missing development package: libpng" >&2
  exit 1
}

work=$(mktemp -d "${TMPDIR:-/tmp}/nes-deck-tests.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM HUP

compile_cpp_test() {
  local source=$1
  local output=$2
  shift 2
  "$cxx" -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
    "$source" "$@" -o "$work/$output"
  "$work/$output"
}

compile_cpp_test tests/nes_audio_test.cpp nes-audio-test -Isrc
compile_cpp_test tests/nes_apu_noise_test.cpp nes-apu-noise-test -Isrc
compile_cpp_test tests/nes_sram_test.cpp nes-sram-test -Isrc
compile_cpp_test tests/joypad_input_test.cpp joypad-input-test -pthread

png_flags=$(pkg-config --cflags --libs libpng)
# pkg-config output is intentionally split into compiler arguments.
# shellcheck disable=SC2086
"$cxx" -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
  src/deck_menu.cpp $png_flags -o "$work/deck-menu-host"
"$work/deck-menu-host" --geometry-test
# shellcheck disable=SC2086
"$cxx" -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
  tests/deck_menu_test.cpp $png_flags -o "$work/deck-menu-test"
"$work/deck-menu-test"

tests/rom_library_test.sh
tests/fetch_covers_test.sh
tests/deck_wifi_profile_add_test.sh
tests/deck_keyboard_quirks_test.sh
tests/retro_terminal_test.sh

compile_cpp_test tests/deck_runtime_test.cpp deck-runtime-test \
  src/deck_runtime.cpp -pthread
octo_src=$(nix eval --raw --impure --expr \
  '(builtins.getFlake ("path:" + toString ./.)).inputs."c-octo-src".outPath')
"$cc" -std=c99 -O2 -Wall -Wextra -Werror -I"$octo_src/src" \
  tests/chip8_core_test.c src/chip8_core.c -o "$work/chip8-core-test"
"$work/chip8-core-test"

(cd uploader && nix shell nixpkgs#go -c go test ./...)

echo "All host tests passed."
