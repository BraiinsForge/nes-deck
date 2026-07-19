#!/usr/bin/env bash

set -euo pipefail
export LC_ALL=C

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
vendor_root=$root/vendor/emulators

for emulator in "$vendor_root"/*/; do
  emulator=${emulator%/}
  [[ -d $emulator && ! -L $emulator ]] || {
    echo "Unsafe emulator vendor directory: $emulator" >&2
    exit 1
  }
  for required in provenance.md LICENSE.txt SHA256SUMS patches/series; do
    [[ -f $emulator/$required && ! -L $emulator/$required ]] || {
      echo "Missing emulator vendor record: $emulator/$required" >&2
      exit 1
    }
  done
  (cd "$emulator" && sha256sum -c SHA256SUMS >/dev/null)

  while IFS= read -r patch || [[ -n $patch ]]; do
    [[ -z $patch || $patch == \#* ]] && continue
    [[ $patch != */* && $patch == *.patch ]] || {
      echo "Invalid patch-series entry in $emulator: $patch" >&2
      exit 1
    }
    [[ -f $emulator/patches/$patch && ! -L $emulator/patches/$patch ]] || {
      echo "Missing emulator patch in $emulator: $patch" >&2
      exit 1
    }
  done <"$emulator/patches/series"
done

echo "vendor-emulators-test: OK"
