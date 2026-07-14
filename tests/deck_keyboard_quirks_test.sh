#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
HELPER=$ROOT/deploy/menu/deck-keyboard-quirks
FIXTURE=$(mktemp -d /tmp/deck-keyboard-quirks-test.XXXXXX)
trap 'rm -rf "$FIXTURE"' EXIT INT TERM HUP

fail() {
	echo "deck-keyboard-quirks-test: $*" >&2
	exit 1
}

mkdir -p "$FIXTURE/devices/2-1.3" \
	"$FIXTURE/devices/2-1.3:1.0" \
	"$FIXTURE/devices/2-1.3:1.2" \
	"$FIXTURE/devices/2-1.4" \
	"$FIXTURE/devices/2-1.4:1.2" \
	"$FIXTURE/drivers/usbhid"
printf '%s\n' 4653 > "$FIXTURE/devices/2-1.3/idVendor"
printf '%s\n' 0001 > "$FIXTURE/devices/2-1.3/idProduct"
printf '%s\n' Mechboards > "$FIXTURE/devices/2-1.3/manufacturer"
printf '%s\n' Corne > "$FIXTURE/devices/2-1.3/product"
printf '%s\n' 00 > "$FIXTURE/devices/2-1.3:1.0/bInterfaceNumber"
printf '%s\n' 02 > "$FIXTURE/devices/2-1.3:1.2/bInterfaceNumber"
ln -s "$FIXTURE/drivers/usbhid" "$FIXTURE/devices/2-1.3:1.0/driver"
ln -s "$FIXTURE/drivers/usbhid" "$FIXTURE/devices/2-1.3:1.2/driver"

# A device with the same USB IDs but a different product name must be left
# alone.
printf '%s\n' 4653 > "$FIXTURE/devices/2-1.4/idVendor"
printf '%s\n' 0001 > "$FIXTURE/devices/2-1.4/idProduct"
printf '%s\n' Mechboards > "$FIXTURE/devices/2-1.4/manufacturer"
printf '%s\n' Other > "$FIXTURE/devices/2-1.4/product"
printf '%s\n' 02 > "$FIXTURE/devices/2-1.4:1.2/bInterfaceNumber"
ln -s "$FIXTURE/drivers/usbhid" "$FIXTURE/devices/2-1.4:1.2/driver"

: > "$FIXTURE/drivers/usbhid/unbind"
RETRO_DECK_USB_DEVICES=$FIXTURE/devices \
RETRO_DECK_USB_DRIVERS=$FIXTURE/drivers \
	"$HELPER"
[ "$(cat "$FIXTURE/drivers/usbhid/unbind")" = '2-1.3:1.2' ] ||
	fail 'helper must detach only Corne interface 02'

# An already detached interface is an idempotent no-op.
rm "$FIXTURE/devices/2-1.3:1.2/driver"
printf '%s' unchanged > "$FIXTURE/drivers/usbhid/unbind"
RETRO_DECK_USB_DEVICES=$FIXTURE/devices \
RETRO_DECK_USB_DRIVERS=$FIXTURE/drivers \
	"$HELPER"
[ "$(cat "$FIXTURE/drivers/usbhid/unbind")" = unchanged ] ||
	fail 'helper must not rewrite an already detached interface'

echo 'deck-keyboard-quirks-test: OK'
