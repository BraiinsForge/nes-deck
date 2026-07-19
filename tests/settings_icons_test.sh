#!/bin/sh

set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
asset_directory=$repo_root/assets/settings-cog
icon=$asset_directory/gear-knekko-09.png

for command in find identify sha256sum; do
	command -v "$command" >/dev/null 2>&1 || {
		echo "Missing required command: $command" >&2
		exit 1
	}
done

[ -f "$icon" ] && [ ! -L "$icon" ] || {
	echo "Approved settings icon is missing or is a symlink" >&2
	exit 1
}
[ "$(find "$asset_directory" -maxdepth 1 -type f -name '*.png' | wc -l)" -eq 1 ] || {
	echo "Settings icon directory must contain exactly one PNG" >&2
	exit 1
}
[ "$(sha256sum "$icon" | cut -d ' ' -f 1)" = \
	92b44756d62e1afaa34c7b1d94cee6f014d5484f94377fe28f4d4392cb696aed ] || {
	echo "Approved settings icon hash changed" >&2
	exit 1
}
[ "$(identify -format '%wx%h' "$icon")" = 23x23 ] || {
	echo "Approved settings icon dimensions changed" >&2
	exit 1
}

echo "settings-icon-test: OK"
