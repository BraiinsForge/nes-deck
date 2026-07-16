#!/bin/sh

set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
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

run_add net1 'fixture-pass-9' >/dev/null
[ -f "$FIXTURE/=6e657431.psk" ] || fail 'canonical hex profile is created'
[ "$(stat -c %a "$FIXTURE")" = 700 ] || fail 'profile directory is private'
[ "$(stat -c %a "$FIXTURE/=6e657431.psk")" = 600 ] || fail 'profile is private'
grep -qx 'Passphrase=fixture-pass-9' "$FIXTURE/=6e657431.psk" ||
	fail 'passphrase is saved exactly'
grep -qx 'AutoConnect=true' "$FIXTURE/=6e657431.psk" ||
	fail 'profile is enabled'
grep -qx '6e657431' "$FIXTURE/preferred" ||
	fail 'new profile is preferred'

printf '%s\n' '[Security]' 'Passphrase=old-plain' >"$FIXTURE/net1.psk"
printf '%s\n' '[Security]' 'Passphrase=old-hex' >"$FIXTURE/=6E657431.psk"
run_add net1 'replacement!9' >/dev/null
[ ! -e "$FIXTURE/net1.psk" ] || fail 'plain duplicate is removed'
[ ! -e "$FIXTURE/=6E657431.psk" ] || fail 'mixed-case hex duplicate is removed'
grep -qx 'Passphrase=replacement!9' "$FIXTURE/=6e657431.psk" ||
	fail 'same SSID is replaced'

run_add Cechomor-Public 'fixture-pass-10' >/dev/null
printf '436563686f6d6f722d5075626c6963\n6e657431\n' > "$FIXTURE/expected-preferred"
cmp "$FIXTURE/expected-preferred" "$FIXTURE/preferred" ||
	fail 'new profiles do not precede older preferences'

before=$(sha256sum "$FIXTURE/=6e657431.psk")
if run_add net1 short >/dev/null 2>&1; then
	fail 'short passphrase is rejected'
fi
[ "$(sha256sum "$FIXTURE/=6e657431.psk")" = "$before" ] ||
	fail 'invalid input does not alter existing profile'

if printf 'net1\nvalidpass9\nextra\n' |
	DECK_WIFI_PROFILE_DIR=$FIXTURE \
	DECK_WIFI_PREFERRED_FILE=$FIXTURE/preferred \
	"$HELPER" >/dev/null 2>&1; then
	fail 'extra input is rejected'
fi

printf '%s\n' 'deck-wifi-profile-add-test: OK'
