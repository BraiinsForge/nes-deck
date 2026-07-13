#!/bin/sh

# Fetch the exact freely licensed NES, GB, GBC, and CHIP-8 builds used by the
# Deck menu.
# Usage: ./ops/deck-menu/fetch-foss-games.sh [output-directory]

set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
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
	"https://github.com/tbsp/Adjustris/releases/download/v1.1/adjustris.gb" \
	"$roms/adjustris.gb" \
	"b6c8affe6d906419cfc99ff459718f33a1868af03254a2f65cea2a9430394712"
fetch_checked \
	"https://raw.githubusercontent.com/AntonioND/geometrix/8f5467ec225e21d67b2e6621eabede70dc6cc8fa/geometrix.gbc" \
	"$roms/geometrix.gbc" \
	"56efdf82118e5faf22511c18dd1fc2ab8bc0c5e44cd634b8e06050ff08124586"
fetch_checked \
	"https://raw.githubusercontent.com/JohnEarnest/chip8Archive/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf/roms/outlaw.ch8" \
	"$roms/outlaw.ch8" \
	"7e45f3eeeafd3cb825f150b51020df4a49212a556e095387382970636c6be0dc"
fetch_checked \
	"https://raw.githubusercontent.com/JohnEarnest/chip8Archive/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf/roms/spaceracer.ch8" \
	"$roms/spaceracer.ch8" \
	"409a67b70a0e7d8bde7e38cc4ec5ceb6570b707bded7541f1682c6e7e53c9b90"

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
fetch_checked \
	"https://raw.githubusercontent.com/tbsp/Adjustris/d899e2f539c07ac49fd42c055605bf778a2df09d/LICENSE.md" \
	"$licenses/adjustris.CC0" \
	"81024528d0d900ab1d46cfbf506056c0ecd4e3ca88ca658db3c4f78d53b57243"
fetch_checked \
	"https://raw.githubusercontent.com/AntonioND/geometrix/8f5467ec225e21d67b2e6621eabede70dc6cc8fa/gpl-3.0.txt" \
	"$licenses/geometrix.GPL-3.0" \
	"8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903"
fetch_checked \
	"https://raw.githubusercontent.com/JohnEarnest/chip8Archive/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf/Readme.md" \
	"$licenses/chip8Archive-CC0-README.md" \
	"f0fd3be302f87da46780bb6c326ebe30d6de39521ca06cd05d63f1292e6fb3f8"

# These sidecars are source-controlled translations of the archive's Octo
# metadata.  chip8-deck applies the exact tick rates, palettes, and the
# two-controller Space Racer mapping when the matching ROM starts.
install -m 0644 "$repo_root/deploy/games/outlaw.ch8.cfg" \
	"$roms/outlaw.ch8.cfg"
install -m 0644 "$repo_root/deploy/games/spaceracer.ch8.cfg" \
	"$roms/spaceracer.ch8.cfg"

echo "Fetched and verified eight FOSS games for NES, GB, GBC, and CHIP-8 in $out"
