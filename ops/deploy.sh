#!/usr/bin/env bash

# Build and install the complete persistent Retro Deck payload.

set -euo pipefail
export LC_ALL=C

usage() {
  echo "Usage: $0 [--config PATH] [--check-config] [root@DECK-IP]" >&2
  exit 2
}

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
activate_script=$script_dir/deploy/activate.sh
config_library=$script_dir/lib/deck-config.sh
config=$repo_root/deck.conf
check_config=0
target_override=

while [[ $# -gt 0 ]]; do
  case $1 in
    --config)
      [[ $# -ge 2 ]] || usage
      config=$2
      shift 2
      ;;
    --check-config)
      check_config=1
      shift
      ;;
    -* ) usage ;;
    *)
      [[ -z $target_override ]] || usage
      target_override=$1
      shift
      ;;
  esac
done

[[ -f $config_library && ! -L $config_library ]] || {
  echo "Deck configuration library is missing or unsafe: $config_library" >&2
  exit 1
}
# shellcheck source=ops/lib/deck-config.sh
source "$config_library"
deck_config_load "$config" "$target_override"
target=$DECK_SSH_TARGET
wireguard_address=$DECK_WIREGUARD_ADDRESS
uploader_password=$ROM_UPLOADER_PASSWORD

if [[ $check_config -eq 1 ]]; then
  echo "Deck configuration is valid for $target at $wireguard_address"
  exit 0
fi

[[ -f $activate_script && ! -L $activate_script ]] || {
  echo "Remote activation script is missing or unsafe: $activate_script" >&2
  exit 1
}

for command in nix ssh tar gzip sha256sum; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done

cd "$repo_root"

work=$(mktemp -d "${TMPDIR:-/tmp}/nes-deck-deploy.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM HUP
payload=$work/payload
foss=$work/foss-games

build_flake() {
  local attribute=$1
  nix build --no-link --print-out-paths "$attribute" | tail -n 1
}

echo "Building static ARM payloads..."
nes=$(build_flake .#nes-deck)
gb=$(build_flake .#gb-deck)
zx=$(build_flake .#zx-deck)
chip8=$(build_flake .#chip8-deck)
timer=$(build_flake .#ten-seconds-deck)
menu=$(build_flake .#deck-menu)
native=$(build_flake .#retrodeck-native)
fbterm=$(build_flake .#fbterm-deck)
rlwrap=$(build_flake .#rlwrap-deck)
lua=$(build_flake .#lua-deck)
python=$(build_flake .#python-deck)
chibi=$(build_flake .#chibi-deck)
chiptune=$(build_flake .#chiptune-deck)
uploader=$(build_flake .#rom-uploader)
runtime_licenses=$(build_flake .#runtime-licenses)
ecl=$(nix build --no-link --print-out-paths -f nix/ecl-arm-static.nix | tail -n 1)

ops/deck-menu/fetch-foss-games.sh "$foss"

mkdir -p \
  "$payload/nes-deck/menu" \
  "$payload/nes-deck/games" \
  "$payload/nes-deck/lisp" \
  "$payload/nes-deck/langs/chibi/lib" \
  "$payload/nes-deck/licenses" \
  "$payload/nes-deck/terminal/fonts" \
  "$payload/nes-deck/terminal/keymaps" \
  "$payload/nes-deck/uploader" \
  "$payload/bmc-widgets/retro-deck/bin" \
  "$payload/nes-deck/ecl" \
  "$payload/chiptunes" \
  "$payload/roms" \
  "$payload/usr/bin" \
  "$payload/usr/sbin" \
  "$payload/etc/init.d"

cp "$nes/bin/nes-deck" "$payload/nes-deck/nes-deck"
cp "$gb/bin/gb-deck" "$payload/nes-deck/gb-deck"
cp "$zx/bin/zx-deck" "$payload/nes-deck/zx-deck"
cp "$chip8/bin/chip8-deck" "$payload/nes-deck/chip8-deck"
cp "$timer/bin/ten-seconds-deck" "$payload/nes-deck/ten-seconds-deck"
cp "$menu/bin/deck-menu" "$payload/nes-deck/menu/deck-menu"
cp "$native/bin/retrodeck-native" "$payload/nes-deck/retrodeck-native"
cp lisp/startup.lisp lisp/ui.lisp lisp/policy.lisp "$payload/nes-deck/lisp/"
cp "$chiptune/bin/chiptune-deck" "$payload/nes-deck/chiptune-deck"
cp "$uploader/bin/rom-uploader" \
  "$payload/nes-deck/uploader/rom-uploader"
printf '%s\n' "$uploader_password" |
  (cd uploader && nix shell nixpkgs#go -c go run . --set-password \
    "$payload/nes-deck/uploader/password.conf")
printf '%s\n' '0.0.0.0:8080' \
  >"$payload/nes-deck/uploader/address.conf"
cp "$lua/bin/lua" "$payload/nes-deck/langs/lua"
cp "$python/bin/python" "$payload/nes-deck/langs/python"
cp "$chibi/bin/chibi-scheme" \
  "$payload/nes-deck/langs/chibi/chibi-scheme"
cp -a "$chibi/share/chibi/." "$payload/nes-deck/langs/chibi/lib/"
cp -a "$ecl/bin" "$ecl/lib" "$payload/nes-deck/ecl/"

cp "$fbterm/bin/fbterm" "$fbterm/bin/loadkeys" \
  "$payload/nes-deck/terminal/"
cp "$rlwrap/bin/rlwrap" "$payload/nes-deck/terminal/"
cp "$fbterm/share/retro-deck/fonts/DejaVuSansMono.ttf" \
  "$payload/nes-deck/terminal/fonts/"
cp -a "$fbterm/share/retro-deck/keymaps/." \
  "$payload/nes-deck/terminal/keymaps/"
cp deploy/terminal/fonts.conf deploy/terminal/retro-terminal \
  "$payload/nes-deck/terminal/"

cp deploy/menu/games.sexp deploy/menu/games.tsv deploy/menu/credits.tsv \
  deploy/menu/palette.tsv \
  deploy/menu/ASSETS.md \
  deploy/menu/compile-catalog.lisp deploy/menu/deck-menu-launcher \
  deploy/menu/fetch-covers "$payload/nes-deck/menu/"
cp assets/settings-cog/gear-knekko-09.png \
  "$payload/nes-deck/menu/settings-icon.png"
cp deploy/widget/manifest.json \
  "$payload/bmc-widgets/retro-deck/manifest.json"
cp deploy/widget/retro-deck \
  "$payload/bmc-widgets/retro-deck/bin/retro-deck"
chmod 0755 "$payload/bmc-widgets/retro-deck/bin/retro-deck"
cp deploy/ecl "$payload/usr/bin/ecl"
cp ops/deck-wifi/deck-wifi-profile-add \
  "$payload/usr/sbin/deck-wifi-profile-add"
cp ops/deck-wifi/deck-wifi-select ops/deck-wifi/deck-wifi-watch \
  "$payload/usr/sbin/"
cp ops/deck-wifi/deck-wifi.init "$payload/etc/init.d/deck-wifi"
cp deploy/menu/nes-deck.init "$payload/etc/init.d/nes-deck"
cp deploy/menu/nes-deck-swap.init "$payload/etc/init.d/nes-deck-swap"
mkdir -p "$payload/etc/hotplug.d/usb"
cp deploy/menu/nes-deck-keyboard.hotplug \
  "$payload/etc/hotplug.d/usb/90-nes-deck-keyboard"
cp deploy/menu/deck-keyboard-quirks \
  "$payload/usr/sbin/deck-keyboard-quirks"
cp deploy/uploader/nes-deck-uploader.init \
  "$payload/etc/init.d/nes-deck-uploader"

for result in "$nes" "$gb" "$zx" "$chip8" "$fbterm" "$rlwrap" "$lua" \
              "$python" "$chibi" "$chiptune" "$runtime_licenses" "$ecl"; do
  if [[ -d $result/share/licenses ]]; then
    cp -a "$result/share/licenses/." "$payload/nes-deck/licenses/"
  fi
done
cp -a "$foss/licenses/." "$payload/nes-deck/licenses/"

if [[ -d chiptunes ]]; then
  find chiptunes -maxdepth 1 -type f \( -name '*.ogg' -o -name '*.ay' -o \
    -name '*.gbs' -o -name '*.gym' -o -name '*.hes' -o -name '*.kss' -o \
    -name '*.nsf' -o -name '*.nsfe' -o -name '*.sap' -o -name '*.spc' -o \
    -name '*.vgm' -o -name '*.vgz' \) -exec cp {} "$payload/chiptunes/" \;
fi

for system in nes gb gbc zx chip8; do
  mkdir -p "$payload/roms/$system"
  if [[ -d roms/$system ]]; then
    cp -a "roms/$system/." "$payload/roms/$system/"
  fi
  if [[ -d $foss/roms/$system ]]; then
    cp -a "$foss/roms/$system/." "$payload/roms/$system/"
  fi
done

find "$payload/nes-deck" -type f \( \
  -name 'nes-deck' -o -name 'gb-deck' -o -name 'zx-deck' -o \
  -name 'chip8-deck' -o -name 'ten-seconds-deck' -o \
  -name 'chiptune-deck' -o -name 'retrodeck-native' -o \
  -name 'deck-menu' -o \
  -name 'deck-menu-launcher' -o -name 'fetch-covers' -o \
  -name 'retro-terminal' -o -name 'fbterm' -o -name 'loadkeys' -o \
  -name 'rlwrap' -o \
  -name 'lua' -o -name 'python' -o -name 'chibi-scheme' -o \
  -name 'ecl.bin' \) -exec chmod 0700 {} +
chmod 0700 "$payload/nes-deck/uploader/rom-uploader"
chmod 0600 "$payload/nes-deck/lisp/startup.lisp" \
  "$payload/nes-deck/lisp/ui.lisp" \
  "$payload/nes-deck/lisp/policy.lisp"
chmod 0600 "$payload/nes-deck/uploader/password.conf"
chmod 0600 "$payload/nes-deck/uploader/address.conf"
chmod 0700 "$payload/usr/bin/ecl" \
  "$payload/usr/sbin/deck-keyboard-quirks" \
  "$payload/usr/sbin/deck-wifi-profile-add" \
  "$payload/usr/sbin/deck-wifi-select" \
  "$payload/usr/sbin/deck-wifi-watch" \
  "$payload/etc/hotplug.d/usb/90-nes-deck-keyboard" \
  "$payload/etc/init.d/deck-wifi" \
  "$payload/etc/init.d/nes-deck" \
  "$payload/etc/init.d/nes-deck-swap" \
  "$payload/etc/init.d/nes-deck-uploader"

remote_stage=/mnt/data/.nes-deck-deploy-$$
echo "Uploading staged payload to $target..."
# The generated staging path is intentionally expanded on the trusted client.
# shellcheck disable=SC2029
ssh "$target" "rm -rf '$remote_stage'; mkdir -p '$remote_stage'; chmod 700 '$remote_stage'"
# shellcheck disable=SC2029
if ! tar -C "$payload" -czf - . | ssh "$target" \
  "gzip -dc | tar -C '$remote_stage' -xf -"; then
  # The generated staging path is constrained above and contains only digits.
  # shellcheck disable=SC2029
  ssh "$target" "rm -rf '$remote_stage'" >/dev/null 2>&1 || :
  echo "Payload transfer failed; removed the remote staging directory" >&2
  exit 1
fi

echo "Validating and activating payload..."
ssh "$target" sh -s -- "$remote_stage" <"$activate_script"

echo
echo "Deployment complete. Verify with:"
printf '  %q --config %q %q\n' "$script_dir/check-deck.sh" "$config" "$target"
