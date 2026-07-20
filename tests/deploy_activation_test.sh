#!/usr/bin/env bash

set -euo pipefail

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
activation=$repo_root/ops/deploy/activate.sh
deployer=$repo_root/ops/deploy.sh
lisp_installer=$repo_root/ops/deploy/install-lisp-tree.sh

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

[[ -x $activation && ! -L $activation ]] ||
  fail 'activation script is not a regular executable'
[[ -x $lisp_installer && ! -L $lisp_installer ]] ||
  fail 'managed Lisp installer is not a regular executable'
sh -n "$activation"
sh -n "$lisp_installer"
bash -n "$deployer"

status=0
output=$(sh "$activation" 2>&1) || status=$?
[[ $status -eq 2 ]] || fail 'missing stage did not produce a usage error'
[[ $output == 'Usage: activate.sh STAGING-DIRECTORY' ]] ||
  fail 'activation usage text changed unexpectedly'

status=0
output=$(sh "$activation" /tmp/not-a-deck-stage 2>&1) || status=$?
[[ $status -eq 1 ]] || fail 'unsafe stage path was accepted'
[[ $output == 'Refusing unexpected staging path: /tmp/not-a-deck-stage' ]] ||
  fail 'unsafe stage path did not produce a specific error'

grep -Fq "activate_script=\$script_dir/deploy/activate.sh" "$deployer" ||
  fail 'deployer does not locate the activation script'
grep -Fq "ssh \"\$target\" sh -s -- \"\$remote_stage\" <\"\$activate_script\"" \
  "$deployer" || fail 'deployer does not stream the activation script'
grep -Fq "[[ -f \$activate_script && ! -L \$activate_script ]]" "$deployer" ||
  fail 'deployer does not validate the activation script'
grep -Fq 'cp lisp/package.lisp lisp/retro-deck.asd lisp/run-worker.lisp' \
  "$deployer" || fail 'deployer does not stage managed Lisp sources'
grep -Fq 'exec /mnt/data/nes-deck/menu/deck-menu-launcher' \
  "$repo_root/deploy/widget/retro-deck" ||
  fail 'the installed widget no longer defaults to the proven launcher'
grep -Fq "\"\$stage/deploy/install-lisp-tree\" --check \"\$stage/nes-deck/lisp\"" \
  "$activation" || fail 'activation does not preflight managed Lisp sources'
grep -Fq "\"\$stage/deploy/install-lisp-tree\" \"\$stage/nes-deck/lisp\" \"\$base/lisp\"" \
  "$activation" || fail 'activation does not preserve and install Lisp policy'

printf 'deploy-activation-test: OK\n'
