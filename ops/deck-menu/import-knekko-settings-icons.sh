#!/bin/sh

# Copy knekko's numbered PNG sprites and record their canonical catalog.

set -eu
export LC_ALL=C

if [ "$#" -ne 3 ]; then
	echo "Usage: $0 EXTRACTED-SOURCE-DIRECTORY ASSET-DIRECTORY OUTPUT.tsv" >&2
	exit 2
fi

source_directory=$1
asset_directory=$2
output=$3

for command in cp identify mktemp sha256sum tr; do
	command -v "$command" >/dev/null 2>&1 || {
		echo "Missing required command: $command" >&2
		exit 1
	}
done

temporary=$(mktemp "${TMPDIR:-/tmp}/knekko-settings-icons.XXXXXX")
trap 'rm -f "$temporary"' EXIT INT TERM HUP

mkdir -p "$asset_directory"
printf '%s\n' \
	'# name<TAB>label<TAB>family<TAB>size<TAB>file<TAB>sha256' >"$temporary"

number=1
while [ "$number" -le 36 ]; do
	padded=$(printf '%02d' "$number")
	case "$number" in
		1|2|3|4|5|6)
			family=small
			directory=Small
			;;
		7|8|9|10|11|12|13|14|15|16)
			family=medium
			directory=Medium
			;;
		*)
			family=large
			directory=Big
			;;
	esac
	image=$source_directory/$directory/$padded.png
	[ -f "$image" ] && [ ! -L "$image" ] || {
		echo "Missing source sprite: $image" >&2
		exit 1
	}
	width=$(identify -format '%w' "$image")
	height=$(identify -format '%h' "$image")
	[ "$width" = "$height" ] && [ "$width" -ge 1 ] && [ "$width" -le 32 ] || {
		echo "Source sprite must be square and at most 32 pixels: $image" >&2
		exit 1
	}
	cp "$image" "$asset_directory/$padded.png"
	hash=$(sha256sum "$asset_directory/$padded.png")
	hash=${hash%% *}
	printf 'gear-knekko-%s\t%s\t%s\t%s\t%s.png\t%s\n' \
		"$padded" "$padded" "$family" "$width" "$padded" "$hash" \
		>>"$temporary"
	number=$((number + 1))
done

tr -d '\r' <"$source_directory/readme.txt" >"$asset_directory/UPSTREAM.txt"
mkdir -p "$(dirname "$output")"
mv "$temporary" "$output"
trap - EXIT INT TERM HUP
