#!/usr/bin/env bash

# Exercise the non-procd swap service without touching host swap state.

set -euo pipefail

if [[ ${NES_DECK_SWAP_TEST_HELPER:-} == 1 ]]; then
  case ${0##*/} in
    mkswap)
      printf '\nFAKE_SWAP_SIGNATURE\n' >>"$1"
      ;;
    swapon)
      grep -q FAKE_SWAP_SIGNATURE "$1" || exit 1
      printf '%s file 1024 0 -2\n' "$1" >>"$TEST_SWAPS"
      ;;
    swapoff)
      awk -v path="$1" 'NR == 1 || $1 != path' "$TEST_SWAPS" \
        >"$TEST_SWAPS.new"
      mv "$TEST_SWAPS.new" "$TEST_SWAPS"
      ;;
    logger)
      printf '%s\n' "$*" >>"$TEST_LOG"
      ;;
    *)
      echo "Unexpected swap test helper: ${0##*/}" >&2
      exit 1
      ;;
  esac
  exit 0
fi

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/nes-deck-swap-test.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM HUP

export TEST_SWAPS=$work/swaps
export TEST_LOG=$work/log
printf 'Filename Type Size Used Priority\n' >"$TEST_SWAPS"
for helper in mkswap swapon swapoff logger; do
  ln -s "$repo_root/tests/nes_deck_swap_test.sh" "$work/$helper"
done
export NES_DECK_SWAP_TEST_HELPER=1

export NES_DECK_SWAP_FILE=$work/state/retro-deck.swap
export NES_DECK_SWAP_SIZE_MIB=1
export NES_DECK_SWAPS_FILE=$TEST_SWAPS
export NES_DECK_MKSWAP_COMMAND=$work/mkswap
export NES_DECK_SWAPON_COMMAND=$work/swapon
export NES_DECK_SWAPOFF_COMMAND=$work/swapoff
export NES_DECK_LOGGER_COMMAND=$work/logger

unset USE_PROCD
# shellcheck disable=SC1091
source "$repo_root/deploy/menu/nes-deck-swap.init"

if [[ ${USE_PROCD+x} ]]; then
  echo 'Swap service must use rc.common start and stop functions' >&2
  exit 1
fi
if ((START <= 90 || START >= 95)); then
  echo 'Swap service must start after data mount and before BMC' >&2
  exit 1
fi

start
[[ -f $NES_DECK_SWAP_FILE ]]
[[ $(stat -c '%a' "$NES_DECK_SWAP_FILE") == 600 ]]
[[ $(stat -c '%a' "${NES_DECK_SWAP_FILE%/*}") == 700 ]]
[[ $(stat -c '%s' "$NES_DECK_SWAP_FILE") -ge 1048576 ]]
[[ $(grep -c "^$NES_DECK_SWAP_FILE " "$TEST_SWAPS") -eq 1 ]]

start
[[ $(grep -c "^$NES_DECK_SWAP_FILE " "$TEST_SWAPS") -eq 1 ]]

stop
if grep -q "^$NES_DECK_SWAP_FILE " "$TEST_SWAPS"; then
  echo 'Swap service did not disable its active swapfile' >&2
  exit 1
fi

invalid=$work/state/invalid.swap
printf 'do not replace me\n' >"$invalid"
NES_DECK_SWAP_FILE=$invalid
# Used by the sourced service functions.
# shellcheck disable=SC2034
SWAP_FILE=$invalid
if start; then
  echo 'Swap service accepted an invalid existing swapfile' >&2
  exit 1
fi
[[ $(cat "$invalid") == 'do not replace me' ]]

echo 'nes-deck-swap-test: OK'
