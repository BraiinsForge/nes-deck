#!/usr/bin/env bash

# Build and run every host-side regression test without polluting the repo.

set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
cd "$repo_root"

cxx=${CXX:-g++}
cc=${CC:-cc}
cargo=${CARGO:-cargo}
sbcl=${SBCL:-sbcl}
for command in "$cxx" "$cc" "$cargo" "$sbcl" montage nix pkg-config; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done
pkg-config --exists libpng || {
  echo "Missing development package: libpng" >&2
  exit 1
}
pkg-config --exists wayland-client || {
  echo "Missing development package: wayland-client" >&2
  exit 1
}

work=$(mktemp -d "${TMPDIR:-/tmp}/nes-deck-tests.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM HUP

CARGO_TARGET_DIR="$work/cargo-target" \
  "$cargo" test --manifest-path native/Cargo.toml --locked --lib
"$sbcl" --noinform --disable-debugger --script tests/lisp_policy_test.lisp

compile_cpp_test() {
  local source=$1
  local output=$2
  shift 2
  "$cxx" -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
    "$source" "$@" -o "$work/$output"
  "$work/$output"
}

compile_cpp_test tests/nes_sram_test.cpp nes-sram-test -Isrc
compile_cpp_test tests/joypad_input_test.cpp joypad-input-test -pthread
compile_cpp_test tests/joypad_input_zx_test.cpp joypad-input-zx-test -pthread

fuse_src=$(nix eval --raw --impure --expr \
  '(builtins.getFlake ("path:" + toString ./.)).inputs."fuse-src".outPath')
compile_cpp_test tests/zx_keyboard_test.cpp zx-keyboard-test \
  -Isrc -I"$fuse_src/src"
compile_cpp_test tests/menu_text_test.cpp menu-text-test src/menu_text.cpp
compile_cpp_test tests/menu_catalog_test.cpp menu-catalog-test \
  src/menu_catalog.cpp src/menu_io.cpp src/menu_text.cpp
compile_cpp_test tests/menu_network_test.cpp menu-network-test \
  src/menu_network.cpp src/menu_text.cpp
compile_cpp_test tests/menu_state_test.cpp menu-state-test \
  src/menu_state.cpp src/menu_io.cpp src/menu_text.cpp
compile_cpp_test tests/menu_ui_test.cpp menu-ui-test \
  src/menu_ui.cpp src/menu_text.cpp
"$cxx" -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
  tests/menu_credits_test.cpp src/menu_credits.cpp src/menu_ui.cpp \
  src/menu_text.cpp \
  -o "$work/menu-credits-test"
"$work/menu-credits-test" "$repo_root/deploy/menu/credits.tsv"

png_flags=$(pkg-config --cflags --libs libpng)
# pkg-config output is intentionally split into compiler arguments.
# shellcheck disable=SC2086
"$cxx" -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
  src/deck_menu.cpp src/menu_sound.cpp src/menu_catalog.cpp \
  src/menu_credits.cpp src/menu_io.cpp src/menu_network.cpp \
  src/menu_state.cpp src/menu_text.cpp src/menu_ui.cpp \
  $png_flags \
  -o "$work/deck-menu-host"
"$work/deck-menu-host" --geometry-test
# shellcheck disable=SC2086
"$cxx" -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
  tests/deck_menu_test.cpp src/menu_sound.cpp src/menu_catalog.cpp \
  src/menu_credits.cpp src/menu_io.cpp src/menu_network.cpp \
  src/menu_state.cpp src/menu_text.cpp src/menu_ui.cpp $png_flags \
  -o "$work/deck-menu-test"
"$work/deck-menu-test"

tests/rom_library_test.sh
tests/catalog_test.sh
tests/licenses_test.sh
tests/render_screenshots_test.sh
tests/fetch_covers_test.sh
tests/settings_icons_test.sh
tests/deploy_config_test.sh
tests/deploy_activation_test.sh
tests/check_deck_test.sh
tests/provision_config_test.sh
tests/deck_wifi_profile_add_test.sh
tests/deck_wifi_select_test.sh
tests/deck_keyboard_quirks_test.sh
tests/retro_terminal_test.sh
tests/nes_deck_swap_test.sh

compile_cpp_test tests/deck_runtime_test.cpp deck-runtime-test \
  src/deck_runtime.cpp -pthread

wayland_scanner=${WAYLAND_SCANNER:-wayland-scanner}
command -v "$wayland_scanner" >/dev/null 2>&1 || {
  echo "Missing required command: $wayland_scanner" >&2
  exit 1
}
"$wayland_scanner" client-header protocol/deck-widget-v1.xml \
  "$work/deck-widget-v1-client-protocol.h"
"$wayland_scanner" private-code protocol/deck-widget-v1.xml \
  "$work/deck-widget-v1-protocol.c"
"$wayland_scanner" client-header protocol/wlr-layer-shell-unstable-v1.xml \
  "$work/wlr-layer-shell-unstable-v1-client-protocol.h"
"$wayland_scanner" private-code protocol/wlr-layer-shell-unstable-v1.xml \
  "$work/wlr-layer-shell-unstable-v1-protocol.c"
wayland_flags=$(pkg-config --cflags --libs wayland-client)
"$cc" -std=c99 -O2 -Wall -Wextra -Werror -I"$work" \
  -c "$work/deck-widget-v1-protocol.c" \
  -o "$work/deck-widget-v1-protocol.o"
"$cc" -std=c99 -O2 -Wall -Wextra -Werror -I"$work" \
  -c "$work/wlr-layer-shell-unstable-v1-protocol.c" \
  -o "$work/wlr-layer-shell-unstable-v1-protocol.o"
# pkg-config output is intentionally split into compiler arguments.
# shellcheck disable=SC2086
"$cxx" -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
  -DRETRO_DECK_WAYLAND=1 -Isrc -I"$work" \
  tests/deck_runtime_test.cpp src/deck_runtime.cpp src/deck_wayland.cpp \
  "$work/deck-widget-v1-protocol.o" \
  "$work/wlr-layer-shell-unstable-v1-protocol.o" \
  $wayland_flags -pthread -o "$work/deck-runtime-wayland-test"
"$work/deck-runtime-wayland-test"
octo_src=$(nix eval --raw --impure --expr \
  '(builtins.getFlake ("path:" + toString ./.)).inputs."c-octo-src".outPath')
"$cc" -std=c99 -O2 -Wall -Wextra -Werror -I"$octo_src/src" \
  tests/chip8_core_test.c src/chip8_core.c -o "$work/chip8-core-test"
"$work/chip8-core-test"

(cd uploader && nix shell nixpkgs#go -c go test ./...)

echo "All host tests passed."
