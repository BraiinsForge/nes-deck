#!/usr/bin/env bash

# Verify the license summaries and the two generated notice archives.

set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
cd "$repo_root"

runtime=$(nix build --no-link --print-out-paths .#runtime-licenses | tail -n 1)
ecl=$(nix build --no-link --print-out-paths -f nix/ecl-arm-static.nix | tail -n 1)

require_notice() {
  local path=$1
  [[ -f $path && ! -L $path && -s $path ]] || {
    echo "Missing generated license notice: $path" >&2
    exit 1
  }
}

for name in \
  Go-LICENSE Nixpkgs-COPYING Wayland-COPYING \
  glibc-COPYING.LIB libffi-LICENSE libpng-LICENSE \
  wlr-layer-shell-LICENSE zlib-LICENSE; do
  require_notice "$runtime/share/licenses/runtime/$name"
done

for name in \
  ASDF-LICENSE Boehm-GC-README.md ECL-COPYING ECL-LICENSE \
  GMP-COPYING.LESSERv3 GMP-COPYINGv2 GMP-README \
  libatomic_ops-LICENSE; do
  require_notice "$ecl/share/licenses/ecl-deck/$name"
done

grep -Fqx $'ECL\tCommon Lisp REPL\tLGPL-2.1-or-later' \
  deploy/menu/credits.tsv
grep -Fqx \
  $'GNU MP\tECL arithmetic\tLGPL-3.0-or-later OR GPL-2.0-or-later' \
  deploy/menu/credits.tsv
grep -Fqx $'Go\twireguard-go runtime\tBSD-3-Clause' \
  deploy/menu/credits.tsv
grep -Fq 'either version 3 of the License' \
  "$ecl/share/licenses/ecl-deck/GMP-README"
grep -Fq 'version 2.1 of the License' \
  "$ecl/share/licenses/ecl-deck/ECL-LICENSE"

echo "licenses_test: OK"
