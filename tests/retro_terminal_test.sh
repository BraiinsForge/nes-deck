#!/bin/sh

set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
LAUNCHER=$ROOT/deploy/terminal/retro-terminal
FIXTURE=$(mktemp -d /tmp/retro-terminal-test.XXXXXX)
trap 'rm -rf "$FIXTURE"' EXIT INT TERM HUP

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

mkdir -p "$FIXTURE/keymaps"
: > "$FIXTURE/fonts.conf"
: > "$FIXTURE/keymaps/us.map"
: > "$FIXTURE/keymaps/cz.map"

cat > "$FIXTURE/loadkeys" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "$LOADKEYS_LOG"
case ${FAIL_CZ_LOAD:-no}:$* in
  yes:*cz.map) exit 1 ;;
esac
EOF
chmod 0700 "$FIXTURE/loadkeys"

cat > "$FIXTURE/fbterm" <<'EOF'
#!/bin/sh
printf '%s\n' "$RETRO_DECK_KEYMAP" > "$FBTERM_LOG"
exit 0
EOF
chmod 0700 "$FIXTURE/fbterm"

LOADKEYS_LOG=$FIXTURE/loadkeys.log
FBTERM_LOG=$FIXTURE/fbterm.log
export LOADKEYS_LOG FBTERM_LOG

RETRO_DECK_TERMINAL_BASE=$FIXTURE \
RETRO_DECK_TERMINAL_TTY=/dev/null \
RETRO_DECK_KEYMAP=cz \
  "$LAUNCHER"

[ "$(sed -n '1p' "$LOADKEYS_LOG")" = \
    "-C /dev/null -q -u $FIXTURE/keymaps/cz.map" ] ||
  fail 'selected Czech keymap is loaded before fbterm'
[ "$(sed -n '2p' "$LOADKEYS_LOG")" = \
    "-C /dev/null -q -u $FIXTURE/keymaps/us.map" ] ||
  fail 'US keymap is restored after fbterm exits'
[ "$(wc -l < "$LOADKEYS_LOG")" -eq 2 ] ||
  fail 'normal terminal run performs exactly two keymap loads'
[ "$(cat "$FBTERM_LOG")" = cz ] ||
  fail 'fbterm inherits the selected keymap name'

: > "$LOADKEYS_LOG"
if RETRO_DECK_TERMINAL_BASE=$FIXTURE \
  RETRO_DECK_TERMINAL_TTY=/dev/null \
  RETRO_DECK_KEYMAP=de \
  "$LAUNCHER" >/dev/null 2>&1; then
  fail 'unsupported keymap is rejected'
fi
[ ! -s "$LOADKEYS_LOG" ] ||
  fail 'unsupported keymap does not change the console map'

: > "$LOADKEYS_LOG"
if FAIL_CZ_LOAD=yes \
  RETRO_DECK_TERMINAL_BASE=$FIXTURE \
  RETRO_DECK_TERMINAL_TTY=/dev/null \
  RETRO_DECK_KEYMAP=cz \
  "$LAUNCHER"; then
  fail 'terminal launch stops when the selected keymap cannot load'
fi
[ "$(sed -n '2p' "$LOADKEYS_LOG")" = \
    "-C /dev/null -q -u $FIXTURE/keymaps/us.map" ] ||
  fail 'US keymap is restored after a selected-keymap load failure'

printf '%s\n' 'retro-terminal-test: OK'
