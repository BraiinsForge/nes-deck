#!/bin/sh

# Render the native dashboard at its exact 1280x480 logical resolution.
# Usage: render-screenshots.sh CATALOG.tsv COVER-DIRECTORY OUTPUT-DIRECTORY

set -eu

if [ "$#" -ne 3 ]; then
	echo "Usage: $0 CATALOG.tsv COVER-DIRECTORY OUTPUT-DIRECTORY" >&2
	exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
catalog=$(realpath "$1")
covers=$(realpath "$2")
output=$3
temporary=$(mktemp "${TMPDIR:-/tmp}/retro-deck-screens.XXXXXX")
staging=$(mktemp -d "${TMPDIR:-/tmp}/retro-deck-screen-output.XXXXXX")
trap 'rm -f "$temporary"; rm -rf "$staging"' EXIT INT TERM

g++ -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
	"$script_dir/render-screenshots.cpp" -lpng -lz -pthread -o "$temporary"
"$temporary" "$catalog" "$covers" "$staging"

montage "$staging"/??-*.png -thumbnail 480x180 -tile 2x \
	-geometry 500x200+10+10 -background '#000000' \
	"$staging/00-overview.png"

mkdir -p "$output"
find "$output" -maxdepth 1 -type f -name '*.png' -delete
cp "$staging"/*.png "$output"/
