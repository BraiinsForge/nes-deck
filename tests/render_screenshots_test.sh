#!/usr/bin/env bash

# Exercise the production screenshot renderer and its extracted UI modules.

set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/retro-deck-screens-test.XXXXXX")
trap 'rm -rf "$fixture"' EXIT INT TERM HUP

mkdir -p "$fixture/covers" "$fixture/output"
"$repo_root/ops/deck-menu/render-screenshots.sh" \
  "$repo_root/deploy/menu/games.tsv" "$fixture/covers" "$fixture/output"

intro=$(find "$fixture/output" -maxdepth 1 -type f \
  -name '*-foss-credits-intro.png' -print -quit)
crawl=$(find "$fixture/output" -maxdepth 1 -type f \
  -name '*-foss-credits-crawl.png' -print -quit)
static=$(find "$fixture/output" -maxdepth 1 -type f \
  -name '*-foss-credits-static.png' -print -quit)
[[ -s $fixture/output/00-overview.png && -s $intro && -s $crawl &&
   -s $static ]] || {
  echo "Screenshot renderer omitted the FOSS crawl or contact sheet" >&2
  exit 1
}
cmp -s "$intro" "$crawl" && {
  echo "Intro and mid-crawl screenshots must show different frames" >&2
  exit 1
}

echo "render-screenshots-test: OK"
