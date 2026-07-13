#!/bin/sh

set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)

for pattern in '*.nes' '*.NES' '*.gb' '*.GB' '*.gbc' '*.GBC' \
               '*.ch8' '*.CH8' '*.zip' '*.ZIP'; do
	for intake in "$repo_root"/$pattern; do
		[ -e "$intake" ] || continue
		echo "unfiled ROM intake at repository root: ${intake##*/}" >&2
		exit 1
	done
done

(cd "$repo_root/roms" && sha256sum -c SHA256SUMS)

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

awk '{print $2}' "$repo_root/roms/SHA256SUMS" | sort > "$temporary/expected"
find "$repo_root/roms" -mindepth 2 -maxdepth 2 -type f \
	\( -name '*.nes' -o -name '*.gb' -o -name '*.gbc' -o -name '*.ch8' \) \
	-printf '%P\n' | sort > "$temporary/actual"
if ! cmp -s "$temporary/expected" "$temporary/actual"; then
	echo "roms/SHA256SUMS and the filed ROM tree differ" >&2
	diff -u "$temporary/expected" "$temporary/actual" >&2 || true
	exit 1
fi

tab=$(printf '\t')
while IFS="$tab" read -r id title system path color; do
	case $system in
		nes|gb|gbc)
			prefix=/mnt/data/roms/$system/
			case $path in
				"$prefix"*) relative=${path#"$prefix"} ;;
				*) echo "$id has a noncanonical $system path: $path" >&2; exit 1 ;;
			esac
			if [ ! -f "$repo_root/roms/$system/$relative" ]; then
				echo "$id is missing roms/$system/$relative" >&2
				exit 1
			fi
			;;
		chip8)
			case $path in /mnt/data/roms/chip8/*.ch8) ;; *) exit 1 ;; esac
			;;
		deck)
			case $path in /mnt/data/nes-deck/games/*) ;; *) exit 1 ;; esac
			;;
		*) echo "$id has unsupported system $system" >&2; exit 1 ;;
	esac
done < "$repo_root/deploy/menu/games.tsv"

echo "rom-library-test: OK"
