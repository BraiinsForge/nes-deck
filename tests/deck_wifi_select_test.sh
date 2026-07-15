#!/bin/sh

set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
selector=$repo_root/ops/deck-wifi/deck-wifi-select
fixture=$(mktemp -d /tmp/deck-wifi-select-test.XXXXXX)
trap 'rm -rf "$fixture"' EXIT HUP INT TERM

fail() {
	echo "deck-wifi-select-test: $*" >&2
	exit 1
}

mkdir -p "$fixture/bin"

cat > "$fixture/bin/iw" <<'FAKE_IW'
#!/bin/sh
if [ "${3:-}" = link ]; then
	if [ "$(cat "$TEST_ASSOCIATED")" = 1 ]; then
		echo 'Connected to 00:11:22:33:44:55 (on wlan0)'
	fi
	exit 0
fi
if [ "${3:-}" = scan ]; then
	cat "$TEST_SCAN"
	exit 0
fi
exit 1
FAKE_IW

cat > "$fixture/bin/ip" <<'FAKE_IP'
#!/bin/sh
if [ "$(cat "$TEST_READY")" != 1 ]; then
	exit 0
fi
case "$*" in
	'-4 address show dev wlan0') echo '    inet 192.0.2.20/24 scope global wlan0' ;;
	'-4 route show default') echo 'default via 192.0.2.1 dev wlan0' ;;
esac
FAKE_IP

cat > "$fixture/bin/uci" <<'FAKE_UCI'
#!/bin/sh
config_directory=
while [ "$#" -gt 0 ]; do
	case $1 in
		-c) config_directory=$2; shift 2 ;;
		-q) shift ;;
		*) break ;;
	esac
done
command=$1
argument=${2:-}
if [ -n "$config_directory" ]; then
	config=$config_directory/wireless
else
	config=$DECK_WIFI_CONFIG
fi
case $command in
	get)
		case $argument in
			wireless.@wifi-iface\[0\]) echo wifi-iface ;;
			wireless.@wifi-iface\[1\]) exit 1 ;;
			*)
				property=${argument##*.}
				awk -F= -v property="$property" '$1 == property { print substr($0, index($0, "=") + 1); found=1; exit } END { if (!found) exit 1 }' "$config"
				;;
		esac
		;;
	set)
		property_expression=${argument%%=*}
		property=${property_expression##*.}
		value=${argument#*=}
		temporary=$config.tmp.$$
		awk -F= -v property="$property" -v value="$value" '
			$1 == property { print property "=" value; found=1; next }
			{ print }
			END { if (!found) print property "=" value }
		' "$config" > "$temporary"
		mv "$temporary" "$config"
		;;
	commit) ;;
	*) exit 1 ;;
esac
FAKE_UCI

cat > "$fixture/bin/wifi" <<'FAKE_WIFI'
#!/bin/sh
ssid=$(awk -F= '$1 == "ssid" { print substr($0, index($0, "=") + 1); exit }' "$DECK_WIFI_CONFIG")
printf '%s\n' "$ssid" >> "$TEST_ATTEMPTS"
if [ "$ssid" = "$TEST_GOOD_SSID" ]; then
	printf '1\n' > "$TEST_ASSOCIATED"
	printf '1\n' > "$TEST_READY"
else
	printf '0\n' > "$TEST_ASSOCIATED"
	printf '0\n' > "$TEST_READY"
fi
FAKE_WIFI

cat > "$fixture/bin/logger" <<'FAKE_LOGGER'
#!/bin/sh
exit 0
FAKE_LOGGER

cat > "$fixture/bin/sleep" <<'FAKE_SLEEP'
#!/bin/sh
exit 0
FAKE_SLEEP

cat > "$fixture/bin/date" <<'FAKE_DATE'
#!/bin/sh
echo 20260715-120000
FAKE_DATE

chmod 0700 "$fixture/bin/iw" "$fixture/bin/ip" "$fixture/bin/uci" \
	"$fixture/bin/wifi" "$fixture/bin/logger" "$fixture/bin/sleep" \
	"$fixture/bin/date"

make_scenario() {
	name=$1
	scenario=$fixture/$name
	mkdir -p "$scenario/profiles" "$scenario/backups" "$scenario/run" \
		"$scenario/tmp"
	cat > "$scenario/wireless" <<'CONFIG'
mode=sta
disabled=0
ssid=old
key=old-password
encryption=psk2
CONFIG
	cp "$scenario/wireless" "$scenario/original-wireless"
	for ssid in old bad good; do
		cat > "$scenario/profiles/$ssid.psk" <<PROFILE
[Security]
Passphrase=${ssid}-password
[Settings]
AutoConnect=true
PROFILE
	done
	cat > "$scenario/scan" <<'SCAN'
BSS 00:00:00:00:00:01(on wlan0)
	signal: -10.00 dBm
	SSID: old
	Authentication suites: PSK
BSS 00:00:00:00:00:02(on wlan0)
	signal: -20.00 dBm
	SSID: bad
	Authentication suites: PSK
BSS 00:00:00:00:00:03(on wlan0)
	signal: -30.00 dBm
	SSID: good
	Authentication suites: PSK
SCAN
	: > "$scenario/attempts"
	printf '0\n' > "$scenario/associated"
	printf '0\n' > "$scenario/ready"
	printf '%s\n' "$scenario"
}

run_selector() {
	scenario=$1
	good_ssid=$2
	TEST_ASSOCIATED=$scenario/associated \
	TEST_READY=$scenario/ready \
	TEST_SCAN=$scenario/scan \
	TEST_ATTEMPTS=$scenario/attempts \
	TEST_GOOD_SSID=$good_ssid \
	DECK_WIFI_PATH=$fixture/bin:/usr/bin:/bin \
	DECK_WIFI_PROFILE_DIR=$scenario/profiles \
	DECK_WIFI_BACKUP_DIR=$scenario/backups \
	DECK_WIFI_CONFIG=$scenario/wireless \
	DECK_WIFI_RUNTIME_CONFIG=$scenario/runtime.conf \
	DECK_WIFI_STATUS_FILE=$scenario/run/status \
	DECK_WIFI_LOCK_DIR=$scenario/run/lock \
	DECK_WIFI_TMP_DIR=$scenario/tmp \
	DECK_WIFI_SCAN_INTERVAL=0 \
	DECK_WIFI_SWITCH_TIMEOUT=2 \
	DECK_WIFI_ROLLBACK_GRACE=2 \
	DECK_WIFI_HEALTH_INTERVAL=1 \
	export TEST_ASSOCIATED TEST_READY TEST_SCAN TEST_ATTEMPTS TEST_GOOD_SSID \
		DECK_WIFI_PATH DECK_WIFI_PROFILE_DIR DECK_WIFI_BACKUP_DIR \
		DECK_WIFI_CONFIG DECK_WIFI_RUNTIME_CONFIG DECK_WIFI_STATUS_FILE \
		DECK_WIFI_LOCK_DIR DECK_WIFI_TMP_DIR DECK_WIFI_SCAN_INTERVAL \
		DECK_WIFI_SWITCH_TIMEOUT DECK_WIFI_ROLLBACK_GRACE \
		DECK_WIFI_HEALTH_INTERVAL
	"$selector"
}

success=$(make_scenario success)
run_selector "$success" good || fail 'multi-candidate selection failed'
printf 'bad\ngood\n' > "$fixture/expected-attempts"
cmp "$fixture/expected-attempts" "$success/attempts" ||
	fail 'selector did not try the next visible profile after failure'
grep -qx 'ssid=good' "$success/wireless" ||
	fail 'successful profile was not committed'
grep -qx 'CONNECTED' "$success/run/status" ||
	fail 'success was not exposed in status'
[ "$(find "$success/backups" -type f -name 'wireless.*.before-switch' | wc -l)" -eq 1 ] ||
	fail 'selector did not create exactly one pre-switch backup'

rollback=$(make_scenario rollback)
if run_selector "$rollback" absent; then
	fail 'all-failed candidate set unexpectedly succeeded'
fi
cmp "$rollback/original-wireless" "$rollback/wireless" ||
	fail 'all-failed candidate set did not restore the original config'
grep -qx 'NO KNOWN WIFI CONNECTED' "$rollback/run/status" ||
	fail 'bounded all-failed state was not exposed'

health=$(make_scenario health)
printf '1\n' > "$health/associated"
if TEST_ASSOCIATED=$health/associated TEST_READY=$health/ready \
	TEST_SCAN=$health/scan TEST_ATTEMPTS=$health/attempts TEST_GOOD_SSID=good \
	DECK_WIFI_PATH=$fixture/bin:/usr/bin:/bin \
	DECK_WIFI_PROFILE_DIR=$health/profiles \
	DECK_WIFI_BACKUP_DIR=$health/backups \
	DECK_WIFI_CONFIG=$health/wireless \
	DECK_WIFI_RUNTIME_CONFIG=$health/runtime.conf \
	DECK_WIFI_STATUS_FILE=$health/run/status \
	DECK_WIFI_LOCK_DIR=$health/run/lock \
	"$selector" --health-check; then
	fail 'association without IPv4/default route was treated as healthy'
fi
printf '1\n' > "$health/ready"
TEST_ASSOCIATED=$health/associated TEST_READY=$health/ready \
	TEST_SCAN=$health/scan TEST_ATTEMPTS=$health/attempts TEST_GOOD_SSID=good \
	DECK_WIFI_PATH=$fixture/bin:/usr/bin:/bin \
	DECK_WIFI_PROFILE_DIR=$health/profiles \
	DECK_WIFI_BACKUP_DIR=$health/backups \
	DECK_WIFI_CONFIG=$health/wireless \
	DECK_WIFI_RUNTIME_CONFIG=$health/runtime.conf \
	DECK_WIFI_STATUS_FILE=$health/run/status \
	DECK_WIFI_LOCK_DIR=$health/run/lock \
	"$selector" --health-check || fail 'complete network health was rejected'

echo 'deck-wifi-select-test: OK'
