#!/bin/sh

# Validate and activate a complete Retro Deck staging tree on one Deck.
# This script runs through SSH with the staging directory as its only argument.

set -eu

if [ "$#" -ne 1 ]; then
  echo "Usage: activate.sh STAGING-DIRECTORY" >&2
  exit 2
fi

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
  echo "Staging path is not a real directory: $stage" >&2
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

# Validate the complete payload before stopping a running service.
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
[ -s "$stage/nes-deck/licenses/runtime/Wayland-COPYING" ] && \
  [ -s "$stage/nes-deck/licenses/ecl-deck/ECL-LICENSE" ] || {
  echo "Staged third-party license archive is incomplete" >&2
  exit 1
}
settings_icon=$stage/nes-deck/menu/settings-icon.png
[ -f "$settings_icon" ] && [ ! -L "$settings_icon" ] &&
  [ "$(sha256sum "$settings_icon" | cut -d ' ' -f 1)" = \
    92b44756d62e1afaa34c7b1d94cee6f014d5484f94377fe28f4d4392cb696aed ] || {
  echo "Staged settings icon is missing or corrupt" >&2
  exit 1
}

# Exercise staged interpreters and configuration parsers on the target CPU.
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

# All preflight checks passed. Service interruption begins here.
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

# Install application runtimes and repository-managed assets.
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

mkdir -p "$base/menu" "$base/terminal" "$base/licenses"
cp -Rp "$stage/nes-deck/menu/." "$base/menu/"
rm -rf "$base/menu/settings-icons"
rm -f "$base/menu/knekko-settings-icons.tsv"
rm -rf "$base/games.new"
mkdir -p "$base/games.new"
cp -Rp "$stage/nes-deck/games/." "$base/games.new/"
rm -rf "$base/games"
mv "$base/games.new" "$base/games"
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
    echo "BMC configuration is missing: /etc/bmc_config.json" >&2
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

# Install system entry points and service definitions.
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

# Select the compositor or direct-framebuffer service owner and restart it.
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

# Confirm readiness before discarding the staging tree.
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
