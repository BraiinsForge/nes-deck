#!/usr/bin/env bash

set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
fixture=$(mktemp -d /tmp/retro-deck-catalog-test.XXXXXX)
trap 'rm -rf "$fixture"' EXIT INT TERM HUP

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

compile_catalog() {
  sbcl --noinform --disable-debugger --script \
    "$root/deploy/menu/compile-catalog.lisp" \
    "$1" "$2" "$3" "$fixture/no-override.sexp"
}

compile_catalog "$root/deploy/menu/games.sexp" \
  "$fixture/games.tsv" "$fixture/palette.tsv" >/dev/null
cmp -s "$fixture/games.tsv" "$root/deploy/menu/games.tsv" ||
  fail 'checked-in games.tsv differs from games.sexp'
cmp -s "$fixture/palette.tsv" "$root/deploy/menu/palette.tsv" ||
  fail 'checked-in palette.tsv differs from games.sexp'

sed 's#/mnt/data/nes-deck/games/ten-seconds#/tmp/ten-seconds#' \
  "$root/deploy/menu/games.sexp" >"$fixture/unsafe.sexp"
if compile_catalog "$fixture/unsafe.sexp" "$fixture/unsafe.tsv" \
  "$fixture/unsafe-palette.tsv" >/dev/null 2>&1; then
  fail 'Deck application outside its installed directory was accepted'
fi

echo 'catalog-test: OK'
