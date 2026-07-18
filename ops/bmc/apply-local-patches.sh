#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
	echo "Usage: $0 BMC-MAIN-CHECKOUT" >&2
	exit 2
fi

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_directory/../.." && pwd)
bmc_checkout=$1

if ! git -C "$bmc_checkout" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
	echo "Not a Git checkout: $bmc_checkout" >&2
	exit 1
fi

for patch in "$repository_root"/patches/bmc-*.patch; do
	if git -C "$bmc_checkout" apply --reverse --check "$patch" 2>/dev/null; then
		echo "Already applied: $(basename "$patch")"
		continue
	fi
	git -C "$bmc_checkout" apply --check "$patch"
	git -C "$bmc_checkout" apply "$patch"
	echo "Applied: $(basename "$patch")"
done
