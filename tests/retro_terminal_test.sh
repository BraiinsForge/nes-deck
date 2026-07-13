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
readlink /proc/$$/fd/0 > "$FBTERM_STDIN_LOG"
exit 0
EOF
chmod 0700 "$FIXTURE/fbterm"

LOADKEYS_LOG=$FIXTURE/loadkeys.log
FBTERM_LOG=$FIXTURE/fbterm.log
FBTERM_STDIN_LOG=$FIXTURE/fbterm-stdin.log
TEST_TTY=/dev/zero
export LOADKEYS_LOG FBTERM_LOG FBTERM_STDIN_LOG

RETRO_DECK_TERMINAL_BASE=$FIXTURE \
RETRO_DECK_TERMINAL_TTY=$TEST_TTY \
RETRO_DECK_KEYMAP=cz \
  "$LAUNCHER"

[ "$(sed -n '1p' "$LOADKEYS_LOG")" = \
    "-C $TEST_TTY -q -u $FIXTURE/keymaps/cz.map" ] ||
  fail 'selected Czech keymap is loaded before fbterm'
[ "$(sed -n '2p' "$LOADKEYS_LOG")" = \
    "-C $TEST_TTY -q -u $FIXTURE/keymaps/us.map" ] ||
  fail 'US keymap is restored after fbterm exits'
[ "$(wc -l < "$LOADKEYS_LOG")" -eq 2 ] ||
  fail 'normal terminal run performs exactly two keymap loads'
[ "$(cat "$FBTERM_LOG")" = cz ] ||
  fail 'fbterm inherits the selected keymap name'
[ "$(cat "$FBTERM_STDIN_LOG")" = "$TEST_TTY" ] ||
  fail 'background fbterm retains the explicit console on stdin'

: > "$LOADKEYS_LOG"
if RETRO_DECK_TERMINAL_BASE=$FIXTURE \
  RETRO_DECK_TERMINAL_TTY=$TEST_TTY \
  RETRO_DECK_KEYMAP=de \
  "$LAUNCHER" >/dev/null 2>&1; then
  fail 'unsupported keymap is rejected'
fi
[ ! -s "$LOADKEYS_LOG" ] ||
  fail 'unsupported keymap does not change the console map'

: > "$LOADKEYS_LOG"
if FAIL_CZ_LOAD=yes \
  RETRO_DECK_TERMINAL_BASE=$FIXTURE \
  RETRO_DECK_TERMINAL_TTY=$TEST_TTY \
  RETRO_DECK_KEYMAP=cz \
  "$LAUNCHER" 2>"$FIXTURE/failure.log"; then
  fail 'terminal launch stops when the selected keymap cannot load'
fi
grep -q 'cannot load selected cz keymap' "$FIXTURE/failure.log" ||
  fail 'terminal initialization failure remains visible on inherited stderr'
[ "$(sed -n '2p' "$LOADKEYS_LOG")" = \
    "-C $TEST_TTY -q -u $FIXTURE/keymaps/us.map" ] ||
  fail 'US keymap is restored after a selected-keymap load failure'

printf '%s\n' 'retro-terminal-test: OK'
