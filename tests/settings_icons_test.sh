#!/bin/sh

set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/settings-icons-test.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM HUP

"$repo_root/ops/deck-menu/generate-knekko-settings-icons.sh" \
	"$repo_root/deploy/menu/knekko-settings-icons.tsv" \
	"$repo_root/uploader/settings-icons" \
	"$work/knekko_settings_icons_generated.inc" \
	"$work/knekko_settings_icons_generated.go"

cmp "$repo_root/src/knekko_settings_icons_generated.inc" \
	"$work/knekko_settings_icons_generated.inc"
cmp "$repo_root/uploader/knekko_settings_icons_generated.go" \
	"$work/knekko_settings_icons_generated.go"

echo "settings-icons-test: OK"
