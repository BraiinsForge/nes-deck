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
  unsafe_link=$(find "$emulator" -type l -print -quit)
  [[ -z $unsafe_link ]] || {
    echo "Symlink in emulator vendor directory: $unsafe_link" >&2
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

  diff -u \
    <(awk 'NF && $1 !~ /^#/ { print }' "$emulator/patches/series" | sort) \
    <(find "$emulator/patches" -maxdepth 1 -type f -name '*.patch' \
      -printf '%f\n' | sort) >&2 || {
    echo "Patch series does not exactly cover $emulator/patches" >&2
    exit 1
  }

  diff -u \
    <({
      printf '%s\n' LICENSE.txt
      find "$emulator/patches" -maxdepth 1 -type f -name '*.patch' \
        -printf 'patches/%f\n'
      [[ ! -d $emulator/upstream ]] ||
        (cd "$emulator" && find upstream -type f -printf '%p\n')
    } | sort) \
    <(cut -c67- "$emulator/SHA256SUMS" | sort) >&2 || {
    echo "Checksum manifest does not exactly cover $emulator sources" >&2
    exit 1
  }
done

echo "vendor-emulators-test: OK"
