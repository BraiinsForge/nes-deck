#!/usr/bin/env bash

set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
installer=$root/ops/deploy/install-lisp-tree.sh
fixture=$(mktemp -d /tmp/nes-deck-lisp-deploy-test.XXXXXX)
trap 'rm -rf "$fixture"' EXIT INT TERM HUP
source_tree=$fixture/source
destination=$fixture/nes-deck/lisp

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

mkdir -p "$source_tree/apps" "$source_tree/policy" \
  "$destination/site.d"
for relative in \
  package.lisp retro-deck.asd run-worker.lisp \
  apps/dashboard.lisp apps/defaults.lisp apps/ten-seconds.lisp \
  policy/conditions.lisp policy/hooks.lisp policy/protocol.lisp \
  policy/worker.lisp; do
  printf 'managed %s\n' "$relative" >"$source_tree/$relative"
done
printf '%s\n' '(in-package #:retro-deck)' \
  >"$destination/site.d/90-local.lisp"
printf '%s\n' stale >"$destination/removed-managed-file.lisp"
chmod 0755 "$destination/site.d"
chmod 0644 "$destination/site.d/90-local.lisp"

"$installer" --check "$source_tree"
"$installer" "$source_tree" "$destination"

[[ -f $destination/run-worker.lisp ]] ||
  fail 'managed worker was not installed'
[[ ! -e $destination/removed-managed-file.lisp ]] ||
  fail 'obsolete managed code survived replacement'
grep -qx '(in-package #:retro-deck)' \
  "$destination/site.d/90-local.lisp" ||
  fail 'device-local policy was not preserved'
[[ $(stat -c %a "$destination") == 700 ]] ||
  fail 'managed Lisp directory is not private'
[[ $(stat -c %a "$destination/run-worker.lisp") == 600 ]] ||
  fail 'managed Lisp source is not private'
[[ $(stat -c %a "$destination/site.d") == 700 ]] ||
  fail 'device-local policy directory is not private'
[[ $(stat -c %a "$destination/site.d/90-local.lisp") == 600 ]] ||
  fail 'device-local policy file is not private'

mkdir "$source_tree/site.d"
if "$installer" --check "$source_tree" >/dev/null 2>&1; then
  fail 'staged device-local policy was accepted'
fi
rmdir "$source_tree/site.d"

mv "$source_tree/policy/hooks.lisp" "$fixture/hooks.lisp"
ln -s "$fixture/hooks.lisp" "$source_tree/policy/hooks.lisp"
if "$installer" --check "$source_tree" >/dev/null 2>&1; then
  fail 'symlinked managed policy was accepted'
fi
rm "$source_tree/policy/hooks.lisp"
mv "$fixture/hooks.lisp" "$source_tree/policy/hooks.lisp"

rm -rf "$destination"
ln -s "$fixture" "$destination"
if "$installer" "$source_tree" "$destination" >/dev/null 2>&1; then
  fail 'symlinked destination was accepted'
fi

echo 'deploy-lisp-tree-test: OK'
