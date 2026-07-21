#!/usr/bin/env bash

# Build and run every host-side regression test without polluting the repo.

set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
cd "$repo_root"

"$script_dir/vendor_emulators_test.sh"

for command in cargo nix; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p retro-deck-emulator --all-targets --all-features -- -D warnings
cargo test --workspace

dashboard_manifest=crates/retro-deck-dashboard/Cargo.toml
cargo fmt --manifest-path "$dashboard_manifest" --all --check
cargo clippy --manifest-path "$dashboard_manifest" --workspace --all-targets \
  --no-default-features --features application-wire -- -D warnings
cargo test --manifest-path "$dashboard_manifest" --workspace --all-targets \
  --no-default-features --features application-wire

tests/rom_library_test.sh
tests/catalog_test.sh
tests/licenses_test.sh
tests/fetch_covers_test.sh
tests/retro_deck_refresh_test.sh
tests/settings_icons_test.sh
tests/deploy_config_test.sh
tests/deploy_activation_test.sh
tests/deploy_lisp_tree_test.sh
tests/check_deck_test.sh
tests/provision_config_test.sh
tests/deck_wifi_profile_add_test.sh
tests/deck_wifi_select_test.sh
tests/deck_keyboard_quirks_test.sh
tests/retro_terminal_test.sh
tests/nes_deck_swap_test.sh

echo "All host tests passed."
