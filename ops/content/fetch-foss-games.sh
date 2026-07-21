#!/bin/sh

# Fetch the exact freely licensed CHIP-8 builds used by the Deck menu.
# Usage: ./ops/content/fetch-foss-games.sh [output-directory]

set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
out=${1:-foss-games}
roms="$out/roms"
chip8_roms="$roms/chip8"
licenses="$out/licenses"
mkdir -p "$chip8_roms" "$licenses"

fetch_checked() {
	url=$1
	destination=$2
	expected=$3
	temporary="$destination.part"

	wget -qO "$temporary" "$url"
	actual=$(sha256sum "$temporary" | awk '{print $1}')
	if [ "$actual" != "$expected" ]; then
		rm -f "$temporary"
		echo "checksum mismatch for $url" >&2
		echo "expected $expected, got $actual" >&2
		exit 1
	fi
	mv "$temporary" "$destination"
}

fetch_checked \
	"https://raw.githubusercontent.com/JohnEarnest/chip8Archive/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf/roms/outlaw.ch8" \
	"$chip8_roms/outlaw.ch8" \
	"7e45f3eeeafd3cb825f150b51020df4a49212a556e095387382970636c6be0dc"
fetch_checked \
	"https://raw.githubusercontent.com/JohnEarnest/chip8Archive/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf/roms/spaceracer.ch8" \
	"$chip8_roms/spaceracer.ch8" \
	"409a67b70a0e7d8bde7e38cc4ec5ceb6570b707bded7541f1682c6e7e53c9b90"

fetch_checked \
	"https://raw.githubusercontent.com/JohnEarnest/chip8Archive/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf/Readme.md" \
	"$licenses/chip8Archive-CC0-README.md" \
	"f0fd3be302f87da46780bb6c326ebe30d6de39521ca06cd05d63f1292e6fb3f8"

# These sidecars are source-controlled translations of the archive's Octo
# metadata.  chip8-deck applies the exact tick rates, palettes, and the
# two-controller Space Racer mapping when the matching ROM starts.
install -m 0644 "$repo_root/deploy/games/outlaw.ch8.cfg" \
	"$chip8_roms/outlaw.ch8.cfg"
install -m 0644 "$repo_root/deploy/games/spaceracer.ch8.cfg" \
	"$chip8_roms/spaceracer.ch8.cfg"

echo "Fetched and verified two CHIP-8 games in $out"
