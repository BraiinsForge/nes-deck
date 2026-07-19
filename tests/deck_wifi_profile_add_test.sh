#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
HELPER=$ROOT/ops/deck-wifi/deck-wifi-profile-add
FIXTURE=$(mktemp -d /tmp/deck-wifi-profile-add-test.XXXXXX)
trap 'rm -rf "$FIXTURE"' EXIT INT TERM HUP

fail() {
	printf 'FAIL: %s\n' "$1" >&2
	exit 1
}

run_add() {
	ssid=$1
	password=$2
	printf '%s\n%s\n' "$ssid" "$password" |
		DECK_WIFI_PROFILE_DIR=$FIXTURE \
		DECK_WIFI_PREFERRED_FILE=$FIXTURE/preferred "$HELPER"
}

run_add studio-test 'fixture-pass-9' >/dev/null
[ -f "$FIXTURE/=73747564696f2d74657374.psk" ] || fail 'canonical hex profile is created'
[ "$(stat -c %a "$FIXTURE")" = 700 ] || fail 'profile directory is private'
[ "$(stat -c %a "$FIXTURE/=73747564696f2d74657374.psk")" = 600 ] || fail 'profile is private'
grep -qx 'Passphrase=fixture-pass-9' "$FIXTURE/=73747564696f2d74657374.psk" ||
	fail 'passphrase is saved exactly'
grep -qx 'AutoConnect=true' "$FIXTURE/=73747564696f2d74657374.psk" ||
	fail 'profile is enabled'
grep -qx '73747564696f2d74657374' "$FIXTURE/preferred" ||
	fail 'new profile is preferred'

printf '%s\n' '[Security]' 'Passphrase=old-plain' >"$FIXTURE/studio-test.psk"
printf '%s\n' '[Security]' 'Passphrase=old-hex' >"$FIXTURE/=73747564696F2D74657374.psk"
run_add studio-test 'replacement!9' >/dev/null
[ ! -e "$FIXTURE/studio-test.psk" ] || fail 'plain duplicate is removed'
[ ! -e "$FIXTURE/=73747564696F2D74657374.psk" ] || fail 'mixed-case hex duplicate is removed'
grep -qx 'Passphrase=replacement!9' "$FIXTURE/=73747564696f2d74657374.psk" ||
	fail 'same SSID is replaced'

run_add guest-test 'fixture-pass-10' >/dev/null
printf '67756573742d74657374\n73747564696f2d74657374\n' > "$FIXTURE/expected-preferred"
cmp "$FIXTURE/expected-preferred" "$FIXTURE/preferred" ||
	fail 'new profiles do not precede older preferences'

before=$(sha256sum "$FIXTURE/=73747564696f2d74657374.psk")
if run_add studio-test short >/dev/null 2>&1; then
	fail 'short passphrase is rejected'
fi
[ "$(sha256sum "$FIXTURE/=73747564696f2d74657374.psk")" = "$before" ] ||
	fail 'invalid input does not alter existing profile'

if printf 'studio-test\nvalidpass9\nextra\n' |
	DECK_WIFI_PROFILE_DIR=$FIXTURE \
	DECK_WIFI_PREFERRED_FILE=$FIXTURE/preferred \
	"$HELPER" >/dev/null 2>&1; then
	fail 'extra input is rejected'
fi

printf '%s\n' 'deck-wifi-profile-add-test: OK'
