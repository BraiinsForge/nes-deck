#!/usr/bin/env bash

set -euo pipefail

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
helper=$repo_root/deploy/menu/retro-deck-refresh
fixture=$(mktemp -d "${TMPDIR:-/tmp}/retro-deck-refresh-test.XXXXXX")
trap 'rm -rf "$fixture"' EXIT INT TERM HUP

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

base=$fixture/base
mkdir -p "$base/menu" "$base/uploads" "$fixture/bin"
printf '%s\n' 'base-game' >"$base/menu/games.tsv"
printf '%s\n' 'uploaded-game' >"$base/uploads/games.tsv"

cat >"$fixture/bin/fetch-covers" <<'MOCK'
#!/bin/sh
printf '%s\t%s\n' "$1" "$2" >>"$REFRESH_TEST_FETCH_CALLS"
MOCK
cat >"$fixture/bin/bmc-compositor" <<'MOCK'
#!/bin/sh
printf '%s\n' "$*" >>"$REFRESH_TEST_COMPOSITOR_CALLS"
MOCK
chmod 0700 "$fixture/bin/fetch-covers" "$fixture/bin/bmc-compositor"

export REFRESH_TEST_FETCH_CALLS=$fixture/fetch-calls
export REFRESH_TEST_COMPOSITOR_CALLS=$fixture/compositor-calls
export RETRO_DECK_BASE=$base
export RETRO_DECK_COVER_FETCHER=$fixture/bin/fetch-covers
export RETRO_DECK_COVER_DIRECTORY=$base/covers
export RETRO_DECK_COMPOSITOR_SERVICE=$fixture/bin/bmc-compositor
export RETRO_DECK_REFRESH_LOCK=$fixture/refresh.lock
export RETRO_DECK_REFRESH_LOG=$fixture/refresh.log
export RETRO_DECK_REFRESH_FOREGROUND=1

"$helper" restart

expected_fetches=$(printf '%s\n%s\n' \
  "$base/menu/games.tsv"$'\t'"$base/covers" \
  "$base/uploads/games.tsv"$'\t'"$base/covers")
[[ $(cat "$REFRESH_TEST_FETCH_CALLS") == "$expected_fetches" ]] ||
  fail 'refresh did not cover both the base and uploaded catalogs'
[[ $(cat "$REFRESH_TEST_COMPOSITOR_CALLS") == restart ]] ||
  fail 'refresh did not restart the BMC compositor exactly once'
[[ -f $RETRO_DECK_REFRESH_LOG ]] || fail 'refresh did not create its log'
[[ ! -e $RETRO_DECK_REFRESH_LOCK ]] || fail 'refresh lock survived completion'

rm -f "$REFRESH_TEST_FETCH_CALLS" "$base/uploads/games.tsv"
"$helper" refresh
[[ $(wc -l <"$REFRESH_TEST_FETCH_CALLS") -eq 1 ]] ||
  fail 'missing optional upload catalog was not ignored'

status=0
output=$("$helper" unknown 2>&1) || status=$?
[[ $status -eq 2 ]] || fail 'unknown operation did not return a usage error'
[[ $output == 'Usage: retro-deck-refresh [refresh|restart]' ]] ||
  fail 'unknown operation did not explain the helper contract'

printf 'retro-deck-refresh-test: OK\n'
