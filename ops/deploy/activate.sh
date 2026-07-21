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
profile=/run/current-profile
native_widget=$profile/lib/bmc-widgets/retro-deck
native_application=$profile/lib/bmc-applications/retro-deck

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

compositor_was_running=0
legacy_was_running=0
uploader_was_running=0
activation_complete=0

restore_after_failure() {
  result=$?
  trap - EXIT
  if [ "$result" -ne 0 ] && [ "$activation_complete" -eq 0 ]; then
    echo "Activation failed; restoring the previous presentation owner" >&2
    if [ "$compositor_was_running" -eq 1 ]; then
      /etc/init.d/bmc-compositor restart >/dev/null 2>&1 || :
    else
      /etc/init.d/bmc-compositor stop >/dev/null 2>&1 || :
      if [ "$legacy_was_running" -eq 1 ] && [ -x /etc/init.d/nes-deck ]; then
        /etc/init.d/nes-deck start >/dev/null 2>&1 || :
      fi
    fi
    if [ "$uploader_was_running" -eq 1 ] && \
       [ -x /etc/init.d/nes-deck-uploader ]; then
      /etc/init.d/nes-deck-uploader start >/dev/null 2>&1 || :
    fi
  fi
  rm -rf "$stage" 2>/dev/null || :
  exit "$result"
}
trap restore_after_failure EXIT

require_executable() {
  path=$1
  label=$2
  [ -x "$path" ] || {
    echo "$label is missing: $path" >&2
    exit 1
  }
}

install_file() {
  source=$1
  destination=$2
  mode=$3
  temporary=$destination.retro-deck-new.$$
  rm -f "$temporary"
  cp -p "$source" "$temporary"
  chmod "$mode" "$temporary"
  mv -f "$temporary" "$destination"
}

# The package generation must be complete before any running service stops.
require_executable /etc/init.d/bmc-compositor \
  "BMC compositor service"
[ -s "$native_widget/manifest.json" ] || {
  echo "Native Retro Deck widget manifest is not installed" >&2
  exit 1
}
require_executable "$native_widget/bin/retro-deck" \
  "Native Retro Deck widget"
[ -s "$native_widget/assets/gear-knekko-09.png" ] || {
  echo "Native Retro Deck settings asset is not installed" >&2
  exit 1
}
[ -s "$native_application/manifest.json" ] || {
  echo "Native Retro Deck application manifest is not installed" >&2
  exit 1
}
require_executable "$native_application/bin/retro-deck-launcher" \
  "Native Retro Deck application launcher"
[ -f /etc/bmc_config.json ] && [ ! -L /etc/bmc_config.json ] || {
  echo "BMC configuration is missing or unsafe: /etc/bmc_config.json" >&2
  exit 1
}

# Validate the complete static payload before interrupting any service.
for executable in \
  nes-deck gb-deck zx-deck chip8-deck ten-seconds-deck chiptune-deck; do
  require_executable "$stage/nes-deck/$executable" \
    "Staged executable $executable"
done
for executable in \
  menu/fetch-covers \
  terminal/fbterm terminal/loadkeys terminal/retro-terminal terminal/rlwrap \
  langs/lua langs/python langs/chibi/chibi-scheme ecl/bin/ecl.bin \
  uploader/rom-uploader; do
  require_executable "$stage/nes-deck/$executable" \
    "Staged runtime $executable"
done
require_executable "$stage/usr/sbin/retro-deck-refresh" \
  "Staged native dashboard refresh helper"
require_executable "$stage/usr/sbin/deck-keyboard-quirks" \
  "Staged keyboard quirk helper"
require_executable "$stage/deploy/install-lisp-tree" \
  "Staged managed Lisp installer"
"$stage/deploy/install-lisp-tree" --check "$stage/nes-deck/lisp"

for wifi_executable in deck-wifi-profile-add deck-wifi-select deck-wifi-watch; do
  require_executable "$stage/usr/sbin/$wifi_executable" \
    "Staged Wi-Fi helper $wifi_executable"
done
require_executable "$stage/etc/init.d/deck-wifi" \
  "Staged Wi-Fi service"
require_executable "$stage/etc/init.d/nes-deck-swap" \
  "Staged swap service"
require_executable "$stage/etc/init.d/nes-deck-uploader" \
  "Staged uploader service"
require_executable "$stage/etc/hotplug.d/usb/90-nes-deck-keyboard" \
  "Staged keyboard hotplug hook"
[ -r "$stage/nes-deck/langs/chibi/lib/init-7.scm" ] || {
  echo "Staged Chibi module library is incomplete" >&2
  exit 1
}
[ -s "$stage/nes-deck/menu/games.tsv" ] || {
  echo "Staged menu catalog is empty" >&2
  exit 1
}
[ -s "$stage/nes-deck/menu/games.sexp" ] || {
  echo "Staged Lisp menu catalog is empty" >&2
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

# Exercise staged interpreters and uploader configuration on the target CPU.
python_result=$("$stage/nes-deck/langs/python" -c 'print(6 * 7)')
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

# Install the scene while the current presentation remains available. This
# command is idempotent and preserves every unrelated BMC setting.
"$stage/nes-deck/uploader/rom-uploader" \
  --install-bmc-scene /etc/bmc_config.json

mkdir -p "$base" /mnt/data/roms /mnt/data/langs \
  /mnt/data/chiptunes "$base/langs" "$base/licenses" "$base/log" \
  "$base/state" "$base/uploader" "$base/uploads"

if [ -x /etc/init.d/nes-deck-uploader ] && \
   /etc/init.d/nes-deck-uploader status >/dev/null 2>&1; then
  uploader_was_running=1
  /etc/init.d/nes-deck-uploader stop 2>/dev/null || :
fi
if /etc/init.d/bmc-compositor status >/dev/null 2>&1; then
  compositor_was_running=1
  /etc/init.d/bmc-compositor stop 2>/dev/null || :
fi
if [ -x /etc/init.d/nes-deck ] && \
   /etc/init.d/nes-deck status >/dev/null 2>&1; then
  legacy_was_running=1
  /etc/init.d/nes-deck stop 2>/dev/null || :
fi

# Install application runtimes and repository-managed assets.
install_file "$stage/nes-deck/nes-deck" "$base/nes-deck" 0700
install_file "$stage/nes-deck/gb-deck" "$base/gb-deck" 0700
install_file "$stage/nes-deck/zx-deck" "$base/zx-deck" 0700
install_file "$stage/nes-deck/chip8-deck" "$base/chip8-deck" 0700
install_file "$stage/nes-deck/ten-seconds-deck" \
  "$base/ten-seconds-deck" 0700
install_file "$stage/nes-deck/chiptune-deck" "$base/chiptune-deck" 0700
install_file "$stage/nes-deck/uploader/rom-uploader" \
  "$base/uploader/rom-uploader" 0700
chmod 0700 "$base/uploader" "$base/uploads"
install_file "$uploader_deploy_config" "$base/uploader/password.conf" 0600
install_file "$uploader_address_config" "$base/uploader/address.conf" 0600

for directory in menu games terminal licenses; do
  rm -rf "$base/$directory.new"
  mkdir -p "$base/$directory.new"
  cp -Rp "$stage/nes-deck/$directory/." "$base/$directory.new/"
  rm -rf "$base/$directory"
  mv "$base/$directory.new" "$base/$directory"
done

rm -rf "$base/ecl.new" "$base/langs/chibi.new"
mv "$stage/nes-deck/ecl" "$base/ecl.new"
mv "$stage/nes-deck/langs/chibi" "$base/langs/chibi.new"
rm -rf "$base/ecl" "$base/langs/chibi"
mv "$base/ecl.new" "$base/ecl"
mv "$base/langs/chibi.new" "$base/langs/chibi"
"$stage/deploy/install-lisp-tree" "$stage/nes-deck/lisp" "$base/lisp"
install_file "$stage/nes-deck/langs/lua" "$base/langs/lua" 0700
install_file "$stage/nes-deck/langs/python" "$base/langs/python" 0700

for system in nes gb gbc zx chip8; do
  mkdir -p "/mnt/data/roms/$system"
  cp -Rp "$stage/roms/$system/." "/mnt/data/roms/$system/"
done
cp -Rp "$stage/chiptunes/." /mnt/data/chiptunes/
mkdir -p /mnt/data/langs/lua /mnt/data/langs/lisp \
  /mnt/data/langs/python /mnt/data/langs/scheme /mnt/data/chiptunes
chmod 0700 /mnt/data/langs/lua /mnt/data/langs/lisp \
  /mnt/data/langs/python /mnt/data/langs/scheme /mnt/data/chiptunes

# Install system entry points atomically, including the Wi-Fi watcher files.
# No Wi-Fi profile or wireless configuration is changed here.
install_file "$stage/usr/bin/ecl" /usr/bin/ecl 0700
install_file "$stage/usr/sbin/retro-deck-refresh" \
  /usr/sbin/retro-deck-refresh 0700
install_file "$stage/usr/sbin/deck-keyboard-quirks" \
  /usr/sbin/deck-keyboard-quirks 0700
install_file "$stage/usr/sbin/deck-wifi-profile-add" \
  /usr/sbin/deck-wifi-profile-add 0700
install_file "$stage/usr/sbin/deck-wifi-select" \
  /usr/sbin/deck-wifi-select 0700
install_file "$stage/usr/sbin/deck-wifi-watch" \
  /usr/sbin/deck-wifi-watch 0700
install_file "$stage/etc/init.d/deck-wifi" /etc/init.d/deck-wifi 0700
install_file "$stage/etc/init.d/nes-deck-swap" \
  /etc/init.d/nes-deck-swap 0700
install_file "$stage/etc/init.d/nes-deck-uploader" \
  /etc/init.d/nes-deck-uploader 0700
mkdir -p /etc/hotplug.d/usb
install_file "$stage/etc/hotplug.d/usb/90-nes-deck-keyboard" \
  /etc/hotplug.d/usb/90-nes-deck-keyboard 0700

# Remove the former shell wrapper so BMC has exactly one native package for
# the Retro Deck widget and application identifiers.
rm -rf /mnt/data/bmc-widgets/retro-deck \
  /mnt/data/bmc-applications/retro-deck

if [ -x /etc/init.d/bmc ]; then
  /etc/init.d/bmc stop 2>/dev/null || :
  /etc/init.d/bmc disable 2>/dev/null || :
fi
if [ -x /etc/init.d/nes-deck ]; then
  /etc/init.d/nes-deck stop 2>/dev/null || :
  /etc/init.d/nes-deck disable 2>/dev/null || :
fi
rm -f /etc/rc.d/S??nes-deck /etc/rc.d/K??nes-deck

# Keep a healthy watcher running. Replacing its files atomically lets the
# current process retain its old inode until reboot, so deployment does not
# provoke a wireless scan or selection attempt.
/etc/init.d/deck-wifi enable
/etc/init.d/deck-wifi status >/dev/null 2>&1 || \
  /etc/init.d/deck-wifi start
rm -f /etc/rc.d/S??nes-deck-swap /etc/rc.d/K??nes-deck-swap
/etc/init.d/nes-deck-swap enable
/etc/init.d/nes-deck-swap start
/etc/init.d/bmc-compositor enable
/etc/init.d/bmc-compositor restart
/etc/init.d/nes-deck-uploader enable
/etc/init.d/nes-deck-uploader restart

# BMC spawns each enabled scene at startup and keeps inactive native widgets in
# the prepared lifecycle state. A missing process therefore means startup failed.
attempt=0
while [ "$attempt" -lt 45 ]; do
  if /etc/init.d/bmc-compositor status >/dev/null 2>&1 && \
     /etc/init.d/nes-deck-uploader status >/dev/null 2>&1 && \
     pidof retro-deck >/dev/null 2>&1 && \
     pidof rom-uploader >/dev/null 2>&1; then
    break
  fi
  attempt=$((attempt + 1))
  sleep 1
done

/etc/init.d/bmc-compositor status >/dev/null 2>&1 || {
  echo "BMC compositor did not restart" >&2
  exit 1
}
/etc/init.d/deck-wifi status >/dev/null 2>&1 || {
  echo "Deck Wi-Fi watcher did not start" >&2
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
pidof retro-deck >/dev/null 2>&1 || {
  echo "Retro Deck native widget did not reach its prepared state" >&2
  exit 1
}
require_executable "$native_widget/bin/retro-deck" \
  "Installed native Retro Deck widget"
require_executable "$native_application/bin/retro-deck-launcher" \
  "Installed native Retro Deck application launcher"

activation_complete=1
compositor_was_running=0
legacy_was_running=0
uploader_was_running=0
rm -rf "$stage"
trap - EXIT

# Cover downloads and the resulting compositor restart happen outside both
# HTTP requests and the activation critical path.
/usr/sbin/retro-deck-refresh refresh
echo "Retro Deck native widget, application, and ROM uploader are running."
