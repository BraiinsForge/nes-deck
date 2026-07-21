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
  local override=${4:-$fixture/no-override.sexp}
  sbcl --noinform --disable-debugger --script \
    "$root/deploy/menu/compile-catalog.lisp" \
    "$1" "$2" "$3" "$override"
}

compile_catalog "$root/deploy/menu/games.sexp" \
  "$fixture/games.tsv" "$fixture/palette.tsv" >/dev/null
cmp -s "$fixture/games.tsv" "$root/deploy/menu/games.tsv" ||
  fail 'checked-in games.tsv differs from games.sexp'
cmp -s "$fixture/palette.tsv" "$root/deploy/menu/palette.tsv" ||
  fail 'checked-in palette.tsv differs from games.sexp'

{
  printf '(:version 2\n :palette\n  ('
  first=1
  while IFS=$'\t' read -r role color; do
    if [[ $role == accent ]]; then
      color='#010203'
    fi
    if [[ $first -eq 0 ]]; then
      printf '\n   '
    fi
    printf ':%s "%s"' "$role" "$color"
    first=0
  done <"$root/deploy/menu/palette.tsv"
  printf '))\n'
} >"$fixture/palette-override.sexp"
compile_catalog "$root/deploy/menu/games.sexp" \
  "$fixture/override-games.tsv" "$fixture/override-palette.tsv" \
  "$fixture/palette-override.sexp" >/dev/null
grep -Fxq $'accent\t#010203' "$fixture/override-palette.tsv" ||
  fail 'version 2 palette override was not applied'

sed 's/(:version 2/(:version 3/' "$fixture/palette-override.sexp" \
  >"$fixture/retired-override.sexp"
if compile_catalog "$root/deploy/menu/games.sexp" \
  "$fixture/retired-games.tsv" "$fixture/retired-palette.tsv" \
  "$fixture/retired-override.sexp" >/dev/null 2>&1; then
  fail 'retired version 3 palette override was accepted'
fi

sed 's#/mnt/data/nes-deck/games/ten-seconds#/tmp/ten-seconds#' \
  "$root/deploy/menu/games.sexp" >"$fixture/unsafe.sexp"
if compile_catalog "$fixture/unsafe.sexp" "$fixture/unsafe.tsv" \
  "$fixture/unsafe-palette.tsv" >/dev/null 2>&1; then
  fail 'Deck application outside its installed directory was accepted'
fi

echo 'catalog-test: OK'
