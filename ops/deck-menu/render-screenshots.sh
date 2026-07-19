#!/bin/sh

# Render the native dashboard at its exact 1280x480 logical resolution.
# Usage: render-screenshots.sh CATALOG.tsv COVER-DIRECTORY OUTPUT-DIRECTORY

set -eu

if [ "$#" -ne 3 ]; then
	echo "Usage: $0 CATALOG.tsv COVER-DIRECTORY OUTPUT-DIRECTORY" >&2
	exit 2
fi

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/../.." && pwd)
cxx=${CXX:-g++}
cargo=${CARGO:-cargo}
if [ -n "${CONVERT:-}" ]; then
	image_convert=$CONVERT
elif command -v magick >/dev/null 2>&1; then
	image_convert=magick
else
	image_convert=convert
fi
for command in "$cxx" "$cargo" "$image_convert" montage realpath; do
	command -v "$command" >/dev/null 2>&1 || {
		echo "Missing required command: $command" >&2
		exit 1
	}
done
catalog=$(realpath "$1")
covers=$(realpath "$2")
output=$3
temporary=$(mktemp "${TMPDIR:-/tmp}/retro-deck-screens.XXXXXX")
staging=$(mktemp -d "${TMPDIR:-/tmp}/retro-deck-screen-output.XXXXXX")
trap 'rm -f "$temporary"; rm -rf "$staging"' EXIT INT TERM

"$cxx" -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
	"$script_dir/render-screenshots.cpp" \
	"$repo_root/src/menu_sound.cpp" \
	"$repo_root/src/menu_catalog.cpp" \
	"$repo_root/src/menu_credits.cpp" \
	"$repo_root/src/menu_io.cpp" \
	"$repo_root/src/menu_network.cpp" \
	"$repo_root/src/menu_state.cpp" \
	"$repo_root/src/menu_text.cpp" \
	"$repo_root/src/menu_ui.cpp" \
	-lpng -lz -pthread -o "$temporary"
next_number=$("$temporary" "$catalog" "$covers" \
	"$repo_root/deploy/menu/credits.tsv" "$staging")
timer_name=$(printf '%02d-timer.png' "$next_number")
timer_ppm=$staging/timer.ppm
"$cargo" run --quiet --manifest-path "$repo_root/Cargo.toml" \
	-p retro-deck-apps --bin render-ten-seconds -- "$timer_ppm"
"$image_convert" "$timer_ppm" "$staging/$timer_name"
rm -f "$timer_ppm"

montage "$staging"/??-*.png -thumbnail 480x180 -tile 2x \
	-geometry 500x200+10+10 -background '#000000' \
	"$staging/00-overview.png"

mkdir -p "$output"
find "$output" -maxdepth 1 -type f -name '*.png' -delete
cp "$staging"/*.png "$output"/
