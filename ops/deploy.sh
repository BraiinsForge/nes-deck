#!/usr/bin/env bash

# Build and install the complete persistent Retro Deck payload.

set -euo pipefail

usage() {
  echo "Usage: $0 root@DECK-IP" >&2
  exit 2
}

[[ $# -eq 1 ]] || usage
target=$1
[[ $target =~ ^root@[A-Za-z0-9._:-]+$ ]] || usage

for command in nix ssh scp tar gzip sha256sum; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
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
fbterm=$(build_flake .#fbterm-deck)
rlwrap=$(build_flake .#rlwrap-deck)
lua=$(build_flake .#lua-deck)
python=$(build_flake .#python-deck)
chibi=$(build_flake .#chibi-deck)
chiptune=$(build_flake .#chiptune-deck)
uploader=$(build_flake .#rom-uploader)
ecl=$(nix build --no-link --print-out-paths -f nix/ecl-arm-static.nix | tail -n 1)

ops/deck-menu/fetch-foss-games.sh "$foss"

mkdir -p \
  "$payload/nes-deck/menu" \
  "$payload/nes-deck/games" \
  "$payload/nes-deck/langs/chibi/lib" \
  "$payload/nes-deck/licenses" \
  "$payload/nes-deck/terminal/fonts" \
  "$payload/nes-deck/terminal/keymaps" \
  "$payload/nes-deck/uploader" \
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
cp "$chiptune/bin/chiptune-deck" "$payload/nes-deck/chiptune-deck"
cp "$uploader/bin/rom-uploader" \
  "$payload/nes-deck/uploader/rom-uploader"
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

cp deploy/menu/games.sexp deploy/menu/games.tsv \
  deploy/menu/compile-catalog.lisp deploy/menu/deck-menu-launcher \
  deploy/menu/fetch-covers "$payload/nes-deck/menu/"
cp deploy/ecl "$payload/usr/bin/ecl"
cp ops/deck-wifi/deck-wifi-profile-add \
  "$payload/usr/sbin/deck-wifi-profile-add"
cp deploy/menu/nes-deck.init "$payload/etc/init.d/nes-deck"
mkdir -p "$payload/etc/hotplug.d/usb"
cp deploy/menu/nes-deck-keyboard.hotplug \
  "$payload/etc/hotplug.d/usb/90-nes-deck-keyboard"
cp deploy/menu/deck-keyboard-quirks \
  "$payload/usr/sbin/deck-keyboard-quirks"
cp deploy/uploader/nes-deck-uploader.init \
  "$payload/etc/init.d/nes-deck-uploader"

for result in "$nes" "$gb" "$zx" "$chip8" "$fbterm" "$rlwrap" "$lua" \
              "$python" "$chibi" "$chiptune"; do
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
  -name 'chiptune-deck' -o -name 'deck-menu' -o \
  -name 'deck-menu-launcher' -o -name 'fetch-covers' -o \
  -name 'retro-terminal' -o -name 'fbterm' -o -name 'loadkeys' -o \
  -name 'rlwrap' -o \
  -name 'lua' -o -name 'python' -o -name 'chibi-scheme' -o \
  -name 'ecl.bin' \) -exec chmod 0700 {} +
chmod 0700 "$payload/nes-deck/uploader/rom-uploader"
chmod 0700 "$payload/usr/bin/ecl" \
  "$payload/usr/sbin/deck-keyboard-quirks" \
  "$payload/usr/sbin/deck-wifi-profile-add" \
  "$payload/etc/hotplug.d/usb/90-nes-deck-keyboard" \
  "$payload/etc/init.d/nes-deck" \
  "$payload/etc/init.d/nes-deck-uploader"

remote_stage=/mnt/data/.nes-deck-deploy-$$
echo "Uploading staged payload to $target..."
# The generated staging path is intentionally expanded on the trusted client.
# shellcheck disable=SC2029
ssh "$target" "rm -rf '$remote_stage'; mkdir -p '$remote_stage'; chmod 700 '$remote_stage'"
# shellcheck disable=SC2029
tar -C "$payload" -czf - . | ssh "$target" \
  "gzip -dc | tar -C '$remote_stage' -xf -"

echo "Validating and activating payload..."
ssh "$target" sh -s -- "$remote_stage" <<'REMOTE'
set -eu

stage=$1
base=/mnt/data/nes-deck
case "$stage" in
  /mnt/data/.nes-deck-deploy-*) ;;
  *)
    echo "Refusing unexpected staging path: $stage" >&2
    exit 1
    ;;
esac
[ -d "$stage" ] && [ ! -L "$stage" ] || {
  echo "Deployment staging directory is missing or unsafe" >&2
  exit 1
}
grep -q ' /mnt/data ' /proc/mounts || {
  echo "/mnt/data is not mounted; refusing persistent deployment" >&2
  exit 1
}

service_needs_restart=0
uploader_needs_restart=0
restore_service_after_failure() {
  result=$?
  trap - EXIT
  if [ "$result" -ne 0 ] && [ "$service_needs_restart" -eq 1 ]; then
    echo "Activation failed; restarting Retro Deck" >&2
    /etc/init.d/nes-deck start >/dev/null 2>&1 || :
  fi
  if [ "$result" -ne 0 ] && [ "$uploader_needs_restart" -eq 1 ] && \
     [ -x /etc/init.d/nes-deck-uploader ]; then
    echo "Activation failed; restarting ROM uploader" >&2
    /etc/init.d/nes-deck-uploader start >/dev/null 2>&1 || :
  fi
  exit "$result"
}
trap restore_service_after_failure EXIT

for executable in \
  nes-deck gb-deck zx-deck chip8-deck ten-seconds-deck chiptune-deck; do
  [ -x "$stage/nes-deck/$executable" ] || {
    echo "Staged executable is missing: $executable" >&2
    exit 1
  }
done
for executable in \
  menu/deck-menu menu/deck-menu-launcher menu/fetch-covers \
  terminal/fbterm terminal/loadkeys terminal/retro-terminal terminal/rlwrap \
  langs/lua langs/python langs/chibi/chibi-scheme ecl/bin/ecl.bin \
  uploader/rom-uploader; do
  [ -x "$stage/nes-deck/$executable" ] || {
    echo "Staged runtime is missing: $executable" >&2
    exit 1
  }
done
[ -x "$stage/usr/sbin/deck-keyboard-quirks" ] || {
  echo "Staged keyboard quirk helper is missing" >&2
  exit 1
}
[ -x "$stage/etc/hotplug.d/usb/90-nes-deck-keyboard" ] || {
  echo "Staged keyboard hotplug hook is missing" >&2
  exit 1
}
[ -r "$stage/nes-deck/langs/chibi/lib/init-7.scm" ] || {
  echo "Staged Chibi module library is incomplete" >&2
  exit 1
}
[ -s "$stage/nes-deck/menu/games.tsv" ] || {
  echo "Staged menu catalog is empty" >&2
  exit 1
}

python_result=$(
  "$stage/nes-deck/langs/python" -c 'print(6 * 7)'
)
[ "$python_result" = 42 ] || {
  echo "Staged Python runtime failed its smoke test" >&2
  exit 1
}
scheme_result=$(
  CHIBI_MODULE_PATH="$stage/nes-deck/langs/chibi/lib" \
    "$stage/nes-deck/langs/chibi/chibi-scheme" -q -p '(+ 20 22)'
)
[ "$scheme_result" = 42 ] || {
  echo "Staged Scheme runtime failed its smoke test" >&2
  exit 1
}
"$stage/nes-deck/menu/deck-menu" --help >/dev/null
uploader_test_config=$stage/nes-deck/uploader/password.test.conf
"$stage/nes-deck/uploader/rom-uploader" --init-password \
  "$uploader_test_config" >/dev/null
"$stage/nes-deck/uploader/rom-uploader" --init-password \
  "$uploader_test_config" >/dev/null
rm -f "$uploader_test_config"

mkdir -p "$base" /mnt/data/roms /mnt/data/langs \
  /mnt/data/chiptunes "$base/langs" "$base/licenses" \
  "$base/uploader" "$base/uploads"

if [ -x /etc/init.d/nes-deck-uploader ]; then
  /etc/init.d/nes-deck-uploader stop 2>/dev/null || :
fi
uploader_needs_restart=1

/etc/init.d/nes-deck stop 2>/dev/null || :
service_needs_restart=1

cp -p "$stage/nes-deck/nes-deck" "$base/nes-deck"
cp -p "$stage/nes-deck/gb-deck" "$base/gb-deck"
cp -p "$stage/nes-deck/zx-deck" "$base/zx-deck"
cp -p "$stage/nes-deck/chip8-deck" "$base/chip8-deck"
cp -p "$stage/nes-deck/ten-seconds-deck" "$base/ten-seconds-deck"
cp -p "$stage/nes-deck/chiptune-deck" "$base/chiptune-deck"
cp -p "$stage/nes-deck/uploader/rom-uploader" \
  "$base/uploader/rom-uploader"
chmod 0700 "$base/uploader" "$base/uploads" \
  "$base/uploader/rom-uploader"
"$base/uploader/rom-uploader" --init-password \
  "$base/uploader/password.conf"

mkdir -p "$base/menu" "$base/games" "$base/terminal" "$base/licenses"
cp -p "$stage/nes-deck/menu/"* "$base/menu/"
cp -p "$stage/nes-deck/games/"* "$base/games/"
cp -Rp "$stage/nes-deck/terminal/." "$base/terminal/"
cp -Rp "$stage/nes-deck/licenses/." "$base/licenses/"

rm -rf "$base/ecl.new" "$base/langs/chibi.new"
mv "$stage/nes-deck/ecl" "$base/ecl.new"
mv "$stage/nes-deck/langs/chibi" "$base/langs/chibi.new"
rm -rf "$base/ecl" "$base/langs/chibi"
mv "$base/ecl.new" "$base/ecl"
mv "$base/langs/chibi.new" "$base/langs/chibi"
cp -p "$stage/nes-deck/langs/lua" "$base/langs/lua"
cp -p "$stage/nes-deck/langs/python" "$base/langs/python"

for system in nes gb gbc zx chip8; do
  mkdir -p "/mnt/data/roms/$system"
  cp -Rp "$stage/roms/$system/." "/mnt/data/roms/$system/"
done
cp -Rp "$stage/chiptunes/." /mnt/data/chiptunes/
mkdir -p /mnt/data/langs/lua /mnt/data/langs/lisp \
  /mnt/data/langs/python /mnt/data/langs/scheme /mnt/data/chiptunes
chmod 0700 /mnt/data/langs/lua /mnt/data/langs/lisp \
  /mnt/data/langs/python /mnt/data/langs/scheme /mnt/data/chiptunes

cp -p "$stage/usr/bin/ecl" /usr/bin/ecl
cp -p "$stage/usr/sbin/deck-wifi-profile-add" \
  /usr/sbin/deck-wifi-profile-add
cp -p "$stage/etc/init.d/nes-deck" /etc/init.d/nes-deck
cp -p "$stage/etc/init.d/nes-deck-uploader" \
  /etc/init.d/nes-deck-uploader
chmod 0700 /usr/bin/ecl /usr/sbin/deck-wifi-profile-add \
  /etc/init.d/nes-deck /etc/init.d/nes-deck-uploader

if [ -x /etc/init.d/bmc ]; then
  /etc/init.d/bmc stop 2>/dev/null || :
  /etc/init.d/bmc disable 2>/dev/null || :
fi
/etc/init.d/nes-deck enable
/etc/init.d/nes-deck start
/etc/init.d/nes-deck-uploader enable
/etc/init.d/nes-deck-uploader start

attempt=0
while [ "$attempt" -lt 30 ]; do
  if /etc/init.d/nes-deck status >/dev/null 2>&1 && \
     pidof deck-menu >/dev/null 2>&1 && \
     /etc/init.d/nes-deck-uploader status >/dev/null 2>&1 && \
     pidof rom-uploader >/dev/null 2>&1; then
    break
  fi
  attempt=$((attempt + 1))
  sleep 1
done

/etc/init.d/nes-deck status >/dev/null 2>&1 || {
  echo "Retro Deck service did not start" >&2
  exit 1
}
pidof deck-menu >/dev/null 2>&1 || {
  echo "Retro Deck menu did not reach its ready state" >&2
  tail -n 80 "$base/log/deck-menu.log" >&2 || :
  exit 1
}
/etc/init.d/nes-deck-uploader status >/dev/null 2>&1 || {
  echo "ROM uploader service did not start" >&2
  exit 1
}
pidof rom-uploader >/dev/null 2>&1 || {
  echo "ROM uploader did not reach its ready state" >&2
  exit 1
}

service_needs_restart=0
uploader_needs_restart=0
rm -rf "$stage"
echo "Retro Deck and its WireGuard ROM uploader are running."
tail -n 12 "$base/log/deck-menu.log" || :
REMOTE

echo
echo "Deployment complete. Verify with:"
echo "  ssh $target '/etc/init.d/nes-deck status; /etc/init.d/nes-deck-uploader status; tail -n 40 /mnt/data/nes-deck/log/deck-menu.log'"
