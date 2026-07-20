#!/bin/sh

# Validate and atomically install the managed Common Lisp policy tree.

set -eu

usage() {
  echo "Usage: install-lisp-tree.sh [--check] SOURCE [DESTINATION]" >&2
  exit 2
}

exists() {
  [ -e "$1" ] || [ -L "$1" ]
}

validate_source() {
  source_tree=$1
  [ -d "$source_tree" ] && [ ! -L "$source_tree" ] || {
    echo "Managed Lisp source is not a real directory: $source_tree" >&2
    return 1
  }
  [ -d "$source_tree/apps" ] && [ ! -L "$source_tree/apps" ] &&
    [ -d "$source_tree/policy" ] && [ ! -L "$source_tree/policy" ] || {
    echo "Managed Lisp source directories are incomplete" >&2
    return 1
  }
  if exists "$source_tree/site.d"; then
    echo "Managed Lisp source must not contain device-local site.d" >&2
    return 1
  fi

  for relative in \
    package.lisp retro-deck.asd run-worker.lisp \
    apps/dashboard.lisp apps/defaults.lisp apps/ten-seconds.lisp \
    policy/conditions.lisp policy/hooks.lisp policy/protocol.lisp \
    policy/worker.lisp; do
    file=$source_tree/$relative
    [ -f "$file" ] && [ ! -L "$file" ] && [ -s "$file" ] || {
      echo "Managed Lisp source is missing or unsafe: $relative" >&2
      return 1
    }
  done

  unsafe=$(find "$source_tree" ! -type d ! -type f -print | sed -n '1p')
  [ -z "$unsafe" ] || {
    echo "Managed Lisp source contains an unsafe entry: $unsafe" >&2
    return 1
  }
  file_count=$(find "$source_tree" -type f -print | wc -l | tr -d ' ')
  [ "$file_count" = 10 ] || {
    echo "Managed Lisp source contains unexpected files" >&2
    return 1
  }
}

if [ "$#" -eq 2 ] && [ "$1" = --check ]; then
  validate_source "$2"
  exit 0
fi
[ "$#" -eq 2 ] || usage

source_tree=$1
destination=$2
case "$source_tree:$destination" in
  /*:/*) ;;
  *)
    echo "Managed Lisp paths must be absolute" >&2
    exit 1
    ;;
esac
[ "$destination" != / ] && [ "${destination%/}" = "$destination" ] || {
  echo "Managed Lisp destination is unsafe: $destination" >&2
  exit 1
}
validate_source "$source_tree"

parent=${destination%/*}
[ -d "$parent" ] && [ ! -L "$parent" ] || {
  echo "Managed Lisp destination parent is not a real directory: $parent" >&2
  exit 1
}
if exists "$destination"; then
  [ -d "$destination" ] && [ ! -L "$destination" ] || {
    echo "Managed Lisp destination is not a real directory: $destination" >&2
    exit 1
  }
fi
if exists "$destination/site.d"; then
  [ -d "$destination/site.d" ] && [ ! -L "$destination/site.d" ] || {
    echo "Device-local Lisp site.d is not a real directory" >&2
    exit 1
  }
fi

new=$destination.new
previous=$destination.previous
if ! exists "$destination" && [ -d "$previous" ] && [ ! -L "$previous" ]; then
  mv "$previous" "$destination"
fi
rm -rf "$new"
if exists "$previous"; then
  [ -d "$destination" ] && [ ! -L "$previous" ] || {
    echo "Managed Lisp recovery tree is unsafe: $previous" >&2
    exit 1
  }
  rm -rf "$previous"
fi

rollback=0
# Invoked by the EXIT trap installed immediately below.
# shellcheck disable=SC2329
cleanup() {
  result=$?
  trap - EXIT INT TERM HUP
  rm -rf "$new"
  if [ "$rollback" -eq 1 ] && ! exists "$destination" &&
     [ -d "$previous" ] && [ ! -L "$previous" ]; then
    mv "$previous" "$destination" 2>/dev/null || :
  fi
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

mkdir -p "$new"
cp -Rp "$source_tree/." "$new/"
if [ -d "$destination/site.d" ]; then
  cp -Rp "$destination/site.d" "$new/site.d"
else
  mkdir -p "$new/site.d"
fi
find "$new" -type d -exec chmod 0700 {} \;
find "$new" -type f -exec chmod 0600 {} \;

if [ -d "$destination" ]; then
  mv "$destination" "$previous"
  rollback=1
fi
mv "$new" "$destination"
rollback=0
rm -rf "$previous"

trap - EXIT INT TERM HUP
exit 0
