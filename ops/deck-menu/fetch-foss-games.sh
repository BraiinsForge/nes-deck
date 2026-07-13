#!/bin/sh

# Fetch the exact freely licensed NES ROM builds used by the Deck menu.
# Usage: ./ops/deck-menu/fetch-foss-games.sh [output-directory]

set -eu

out=${1:-foss-games}
roms="$out/roms"
licenses="$out/licenses"
mkdir -p "$roms" "$licenses"

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
	"https://raw.githubusercontent.com/xram64/falling-nes/52dcb8a951200562e696dfc2aba5d4d14edd0078/falling.nes" \
	"$roms/falling.nes" \
	"e22b947542c2d7e595bf84725b333be7af8189c5965b9c53e356a249c7d79943"
fetch_checked \
	"https://github.com/pinobatch/thwaite-nes/releases/download/v0.04/thwaite.nes" \
	"$roms/thwaite.nes" \
	"a2df24d9c9f72e56c2fdc4c703becc47a5700ad0158da8208247635ebeb3779c"
fetch_checked \
	"https://github.com/pinobatch/croom-nes/releases/download/v0.02a/croom.nes" \
	"$roms/croom.nes" \
	"2ce17df1ad66a8a0533c0a8739f5b5ebe275c264924bbe350c42c5ac0394f20e"
fetch_checked \
	"https://github.com/pinobatch/rfk-nes/releases/download/v0.10/robotfindskitten.nes" \
	"$roms/robotfindskitten.nes" \
	"13abbea91f553780c88c2a85a40b7e86fd5916026c01bfc4f88a8b9b9a9abfe1"

fetch_checked \
	"https://raw.githubusercontent.com/xram64/falling-nes/52dcb8a951200562e696dfc2aba5d4d14edd0078/LICENSE" \
	"$licenses/falling.LICENSE" \
	"030010f3b77794b439b35cb072e7097ebeee713cac44b2ec1c532cbed8b94acd"
fetch_checked \
	"https://raw.githubusercontent.com/pinobatch/thwaite-nes/ccd27ccbe3e201755b86c1b5f932b2e11ba74110/LICENSE.txt" \
	"$licenses/thwaite.COPYING" \
	"589ed823e9a84c56feb95ac58e7cf384626b9cbf4fda2a907bc36e103de1bad2"
fetch_checked \
	"https://raw.githubusercontent.com/pinobatch/croom-nes/209e628564ab890d05c188e7c84f80217088aa5a/LICENSE.txt" \
	"$licenses/croom.COPYING" \
	"fc82ca8b6fdb18d4e3e85cfd8ab58d1bcd3f1b29abe782895abd91d64763f8e7"
fetch_checked \
	"https://raw.githubusercontent.com/pinobatch/rfk-nes/4d26698fcce966b6d9e982aa06cbb2cabad4750a/LICENSE" \
	"$licenses/robotfindskitten.LICENSE" \
	"c0f10c38673ee2fa93203c8df7870210722c828016ded2447070b235e1f653f9"

echo "Fetched and verified four FOSS NES games in $out"

