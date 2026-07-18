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

[[ -f $config && ! -L $config ]] || {
  echo "Missing private Deck configuration: $config" >&2
  echo "Create it with: $repo_root/ops/configure-deck.sh $config" >&2
  exit 1
}
config_mode=$(stat -c %a -- "$config")
if (( (8#$config_mode & 077) != 0 )); then
  echo "Deck configuration must not be accessible by group or others: $config" >&2
  exit 1
fi

target=
wireguard_address=
uploader_password=
target_seen=0
wireguard_seen=0
password_seen=0
line_number=0
while IFS= read -r line || [[ -n $line ]]; do
  line_number=$((line_number + 1))
  [[ -z $line || $line == \#* ]] && continue
  [[ $line == *=* ]] || {
    echo "Malformed configuration line $line_number in $config" >&2
    exit 1
  }
  key=${line%%=*}
  value=${line#*=}
  case $key in
    DECK_SSH_TARGET)
      [[ $target_seen -eq 0 ]] || {
        echo "Duplicate DECK_SSH_TARGET in $config" >&2
        exit 1
      }
      target=$value
      target_seen=1
      ;;
    DECK_WIREGUARD_ADDRESS)
      [[ $wireguard_seen -eq 0 ]] || {
        echo "Duplicate DECK_WIREGUARD_ADDRESS in $config" >&2
        exit 1
      }
      wireguard_address=$value
      wireguard_seen=1
      ;;
    ROM_UPLOADER_PASSWORD)
      [[ $password_seen -eq 0 ]] || {
        echo "Duplicate ROM_UPLOADER_PASSWORD in $config" >&2
        exit 1
      }
      uploader_password=$value
      password_seen=1
      ;;
    *)
      echo "Unknown configuration key on line $line_number in $config: $key" >&2
      exit 1
      ;;
  esac
done <"$config"

if [[ -n $target_override ]]; then
  target=$target_override
fi
[[ $target =~ ^root@[A-Za-z0-9._:-]+$ ]] || {
  echo "DECK_SSH_TARGET must have the form root@DECK-IP" >&2
  exit 1
}
if [[ ! $wireguard_address =~ ^10\.0\.0\.([0-9]{1,3})$ ||
      ${BASH_REMATCH[1]} -lt 2 || ${BASH_REMATCH[1]} -gt 253 ]]; then
  echo "DECK_WIREGUARD_ADDRESS must be a usable 10.0.0.0/24 peer address" >&2
  exit 1
fi
if [[ $password_seen -eq 0 || ${#uploader_password} -lt 8 ||
      ${#uploader_password} -gt 128 || $uploader_password == *$'\r'* ||
      $uploader_password == *$'\n'* ]]; then
  echo "ROM_UPLOADER_PASSWORD must contain 8 through 128 bytes without line breaks" >&2
  exit 1
fi

if [[ $check_config -eq 1 ]]; then
  echo "Deck configuration is valid for $target at $wireguard_address"
  exit 0
fi

for command in nix ssh scp tar gzip sha256sum; do
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
  "$payload/nes-deck/menu/settings-icons" \
  "$payload/nes-deck/games" \
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
  deploy/menu/knekko-settings-icons.tsv deploy/menu/ASSETS.md \
  deploy/menu/compile-catalog.lisp deploy/menu/deck-menu-launcher \
  deploy/menu/fetch-covers "$payload/nes-deck/menu/"
cp -a uploader/settings-icons/. "$payload/nes-deck/menu/settings-icons/"
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
bmc_mode=0
compositor_needs_restart=0
if [ -x /etc/init.d/bmc-compositor ]; then
  bmc_mode=1
fi
restore_service_after_failure() {
  result=$?
  trap - EXIT
  if [ "$result" -ne 0 ] && [ "$service_needs_restart" -eq 1 ]; then
    echo "Activation failed; restarting Retro Deck" >&2
    /etc/init.d/nes-deck start >/dev/null 2>&1 || :
  fi
  if [ "$result" -ne 0 ] && \
     [ "$compositor_needs_restart" -eq 1 ]; then
    echo "Activation failed; restarting BMC compositor" >&2
    /etc/init.d/bmc-compositor restart >/dev/null 2>&1 || :
  fi
  if [ "$result" -ne 0 ] && [ "$uploader_needs_restart" -eq 1 ] && \
     [ -x /etc/init.d/nes-deck-uploader ]; then
    echo "Activation failed; restarting ROM uploader" >&2
    /etc/init.d/nes-deck-uploader start >/dev/null 2>&1 || :
  fi
  rm -rf "$stage" 2>/dev/null || :
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
  [ -s "$stage/bmc-widgets/retro-deck/manifest.json" ] && \
  [ -x "$stage/bmc-widgets/retro-deck/bin/retro-deck" ] || {
  echo "Staged Retro Deck widget is incomplete" >&2
  exit 1
}
for wifi_executable in deck-wifi-profile-add deck-wifi-select deck-wifi-watch; do
  [ -x "$stage/usr/sbin/$wifi_executable" ] || {
    echo "Staged Wi-Fi helper is missing: $wifi_executable" >&2
    exit 1
  }
done
[ -x "$stage/etc/init.d/deck-wifi" ] || {
  echo "Staged Wi-Fi service is missing" >&2
  exit 1
}
[ -x "$stage/etc/init.d/nes-deck-swap" ] || {
  echo "Staged swap service is missing" >&2
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
[ -s "$stage/nes-deck/menu/palette.tsv" ] || {
  echo "Staged dashboard palette is empty" >&2
  exit 1
}
[ "$(find "$stage/nes-deck/menu/settings-icons" -maxdepth 1 \
  -type f -name "*.png" | wc -l)" -eq 36 ] || {
  echo "Staged settings icon set is incomplete" >&2
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
"$stage/nes-deck/menu/deck-menu" --validate-palette \
  "$stage/nes-deck/menu/palette.tsv"
uploader_deploy_config=$stage/nes-deck/uploader/password.conf
uploader_address_config=$stage/nes-deck/uploader/address.conf
[ -f "$uploader_deploy_config" ] && [ ! -L "$uploader_deploy_config" ] || {
  echo "Staged uploader password configuration is missing or unsafe" >&2
  exit 1
}
"$stage/nes-deck/uploader/rom-uploader" --check-password-config \
  "$uploader_deploy_config"
[ -f "$uploader_address_config" ] && [ ! -L "$uploader_address_config" ] || {
  echo "Staged uploader address configuration is missing or unsafe" >&2
  exit 1
}
"$stage/nes-deck/uploader/rom-uploader" --check-address \
  "$uploader_address_config"

mkdir -p "$base" /mnt/data/roms /mnt/data/langs \
  /mnt/data/chiptunes "$base/langs" "$base/licenses" \
  "$base/uploader" "$base/uploads" /mnt/data/bmc-widgets

if [ -x /etc/init.d/nes-deck-uploader ]; then
  /etc/init.d/nes-deck-uploader stop 2>/dev/null || :
fi
uploader_needs_restart=1

/etc/init.d/nes-deck stop 2>/dev/null || :
if [ "$bmc_mode" -eq 1 ]; then
  /etc/init.d/bmc-compositor stop 2>/dev/null || :
  compositor_needs_restart=1
else
  service_needs_restart=1
fi

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
cp -p "$uploader_deploy_config" "$base/uploader/password.conf"
cp -p "$uploader_address_config" "$base/uploader/address.conf"
chmod 0600 "$base/uploader/password.conf" "$base/uploader/address.conf"

mkdir -p "$base/menu" "$base/games" "$base/terminal" "$base/licenses"
cp -Rp "$stage/nes-deck/menu/." "$base/menu/"
cp -p "$stage/nes-deck/games/"* "$base/games/"
cp -Rp "$stage/nes-deck/terminal/." "$base/terminal/"
cp -Rp "$stage/nes-deck/licenses/." "$base/licenses/"

rm -rf /mnt/data/bmc-widgets/retro-deck.new
cp -Rp "$stage/bmc-widgets/retro-deck" \
  /mnt/data/bmc-widgets/retro-deck.new
rm -rf /mnt/data/bmc-widgets/retro-deck
mv /mnt/data/bmc-widgets/retro-deck.new \
  /mnt/data/bmc-widgets/retro-deck
if [ "$bmc_mode" -eq 1 ]; then
  [ -f /etc/bmc_config.json ] || {
    echo "BMC configuration is missing" >&2
    exit 1
  }
  "$stage/nes-deck/uploader/rom-uploader" \
    --install-bmc-scene /etc/bmc_config.json
fi

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
cp -p "$stage/usr/sbin/deck-wifi-select" /usr/sbin/deck-wifi-select
cp -p "$stage/usr/sbin/deck-wifi-watch" /usr/sbin/deck-wifi-watch
cp -p "$stage/etc/init.d/deck-wifi" /etc/init.d/deck-wifi
cp -p "$stage/etc/init.d/nes-deck" /etc/init.d/nes-deck
cp -p "$stage/etc/init.d/nes-deck-swap" /etc/init.d/nes-deck-swap
cp -p "$stage/etc/init.d/nes-deck-uploader" \
  /etc/init.d/nes-deck-uploader
chmod 0700 /usr/bin/ecl /usr/sbin/deck-wifi-profile-add \
  /usr/sbin/deck-wifi-select /usr/sbin/deck-wifi-watch \
  /etc/init.d/deck-wifi \
  /etc/init.d/nes-deck /etc/init.d/nes-deck-swap \
  /etc/init.d/nes-deck-uploader

if [ -x /etc/init.d/bmc ]; then
  /etc/init.d/bmc stop 2>/dev/null || :
  /etc/init.d/bmc disable 2>/dev/null || :
fi
/etc/init.d/deck-wifi enable
/etc/init.d/deck-wifi restart
# Remove links created with an older START or STOP value before re-enabling.
rm -f /etc/rc.d/S??nes-deck-swap /etc/rc.d/K??nes-deck-swap
if [ "$bmc_mode" -eq 1 ]; then
  /etc/init.d/nes-deck disable
  /etc/init.d/nes-deck-swap enable
  /etc/init.d/nes-deck-swap start
  service_needs_restart=0
  /etc/init.d/bmc-compositor enable
  /etc/init.d/bmc-compositor start
else
  /etc/init.d/nes-deck-swap stop 2>/dev/null || :
  /etc/init.d/nes-deck-swap disable
  /etc/init.d/nes-deck enable
  /etc/init.d/nes-deck start
fi
/etc/init.d/nes-deck-uploader enable
/etc/init.d/nes-deck-uploader start

# A fresh Deck may need to download and decode the persistent Libretro indexes
# before deck-menu starts. Keep this bounded, but allow the first fill to
# finish on the target CPU instead of rolling back a healthy installation.
attempt=0
while [ "$attempt" -lt 120 ]; do
  dashboard_ready=0
  if [ "$bmc_mode" -eq 1 ] && \
     /etc/init.d/bmc-compositor status >/dev/null 2>&1; then
    dashboard_ready=1
  elif [ "$bmc_mode" -eq 0 ] && \
       /etc/init.d/nes-deck status >/dev/null 2>&1 && \
       pidof deck-menu >/dev/null 2>&1; then
    dashboard_ready=1
  fi
  if [ "$dashboard_ready" -eq 1 ] && \
     /etc/init.d/nes-deck-uploader status >/dev/null 2>&1 && \
     pidof rom-uploader >/dev/null 2>&1; then
    break
  fi
  attempt=$((attempt + 1))
  sleep 1
done

if [ "$bmc_mode" -eq 1 ]; then
  /etc/init.d/bmc-compositor status >/dev/null 2>&1 || {
    echo "BMC compositor did not restart" >&2
    exit 1
  }
else
  /etc/init.d/nes-deck status >/dev/null 2>&1 || {
    echo "Retro Deck service did not start" >&2
    exit 1
  }
fi
/etc/init.d/deck-wifi status >/dev/null 2>&1 || {
  echo "Deck Wi-Fi watcher did not start" >&2
  exit 1
}
if [ "$bmc_mode" -eq 0 ] && ! pidof deck-menu >/dev/null 2>&1; then
  echo "Retro Deck menu did not reach its ready state" >&2
  tail -n 80 "$base/log/deck-menu.log" >&2 || :
  exit 1
fi
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
compositor_needs_restart=0
rm -rf "$stage"
if [ "$bmc_mode" -eq 1 ]; then
  echo "Retro Deck widget is installed and the BMC compositor is running."
else
  echo "Retro Deck and its ROM uploader are running."
fi
tail -n 12 "$base/log/deck-menu.log" || :
REMOTE

echo
echo "Deployment complete. Verify with:"
echo "  ssh $target '/etc/init.d/nes-deck status; /etc/init.d/nes-deck-uploader status; tail -n 40 /mnt/data/nes-deck/log/deck-menu.log'"
