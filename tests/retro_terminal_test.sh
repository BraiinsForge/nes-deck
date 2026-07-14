#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
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
printf '%s\n' "$*" > "$FBTERM_ARGS_LOG"
pwd > "$FBTERM_CWD_LOG"
printf '%s\n' "${ECLDIR:-}" > "$FBTERM_ECLDIR_LOG"
printf '%s\n' "${CHIBI_MODULE_PATH:-}" > "$FBTERM_CHIBI_LOG"
readlink /proc/$$/fd/0 > "$FBTERM_STDIN_LOG"
exit 0
EOF
chmod 0700 "$FIXTURE/fbterm"

LOADKEYS_LOG=$FIXTURE/loadkeys.log
FBTERM_LOG=$FIXTURE/fbterm.log
FBTERM_ARGS_LOG=$FIXTURE/fbterm-args.log
FBTERM_CWD_LOG=$FIXTURE/fbterm-cwd.log
FBTERM_ECLDIR_LOG=$FIXTURE/fbterm-ecldir.log
FBTERM_CHIBI_LOG=$FIXTURE/fbterm-chibi.log
FBTERM_STDIN_LOG=$FIXTURE/fbterm-stdin.log
TEST_TTY=/dev/zero
export LOADKEYS_LOG FBTERM_LOG FBTERM_ARGS_LOG FBTERM_CWD_LOG
export FBTERM_ECLDIR_LOG FBTERM_CHIBI_LOG FBTERM_STDIN_LOG

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
[ "$(cat "$FBTERM_ARGS_LOG")" = \
    "-n DejaVu Sans Mono -s 16 -f 7 -b 0 -- /bin/ash -l" ] ||
  fail 'default terminal mode launches the login shell'

mkdir -p "$FIXTURE/ecl/lib/ecl"
cat > "$FIXTURE/lua" <<'EOF'
#!/bin/sh
exit 0
EOF
cat > "$FIXTURE/lisp" <<'EOF'
#!/bin/sh
exit 0
EOF
cat > "$FIXTURE/rlwrap" <<'EOF'
#!/bin/sh
exit 0
EOF
cat > "$FIXTURE/python" <<'EOF'
#!/bin/sh
exit 0
EOF
cat > "$FIXTURE/scheme" <<'EOF'
#!/bin/sh
exit 0
EOF
mkdir -p "$FIXTURE/chibi"
: > "$FIXTURE/chibi/init-7.scm"
chmod 0700 "$FIXTURE/lua" "$FIXTURE/lisp" "$FIXTURE/rlwrap" "$FIXTURE/python" \
  "$FIXTURE/scheme"

RETRO_DECK_TERMINAL_BASE=$FIXTURE \
RETRO_DECK_TERMINAL_TTY=$TEST_TTY \
RETRO_DECK_KEYMAP=us \
RETRO_DECK_LANG_BASE=$FIXTURE/langs \
RETRO_DECK_LUA=$FIXTURE/lua \
  "$LAUNCHER" lua
[ "$(cat "$FBTERM_ARGS_LOG")" = \
    "-n DejaVu Sans Mono -s 16 -f 7 -b 0 -- $FIXTURE/lua" ] ||
  fail 'Lua mode launches only the configured Lua interpreter'
[ "$(cat "$FBTERM_CWD_LOG")" = "$FIXTURE/langs/lua" ] ||
  fail 'Lua mode uses its persistent working directory'
[ "$(stat -c %a "$FIXTURE/langs/lua")" = 700 ] ||
  fail 'Lua working directory is private'

RETRO_DECK_TERMINAL_BASE=$FIXTURE \
RETRO_DECK_TERMINAL_TTY=$TEST_TTY \
RETRO_DECK_KEYMAP=us \
RETRO_DECK_LANG_BASE=$FIXTURE/langs \
RETRO_DECK_LISP=$FIXTURE/lisp \
RETRO_DECK_ECL_DIR=$FIXTURE/ecl/lib/ecl \
RETRO_DECK_RLWRAP=$FIXTURE/rlwrap \
  "$LAUNCHER" lisp
[ "$(cat "$FBTERM_ARGS_LOG")" = \
    "-n DejaVu Sans Mono -s 16 -f 7 -b 0 -- $FIXTURE/rlwrap -H $FIXTURE/langs/lisp/.ecl_history $FIXTURE/lisp --norc" ] ||
  fail 'Lisp mode wraps the configured ECL interpreter with persistent history'
[ "$(cat "$FBTERM_CWD_LOG")" = "$FIXTURE/langs/lisp" ] ||
  fail 'Lisp mode uses its persistent working directory'
[ "$(cat "$FBTERM_ECLDIR_LOG")" = "$FIXTURE/ecl/lib/ecl" ] ||
  fail 'Lisp mode exports the configured ECL runtime directory'
[ "$(stat -c %a "$FIXTURE/langs/lisp")" = 700 ] ||
  fail 'Lisp working directory is private'
[ "$(stat -c %a "$FIXTURE/langs/lisp/.ecl_history")" = 600 ] ||
  fail 'Lisp history is private'

RETRO_DECK_TERMINAL_BASE=$FIXTURE \
RETRO_DECK_TERMINAL_TTY=$TEST_TTY \
RETRO_DECK_KEYMAP=us \
RETRO_DECK_LANG_BASE=$FIXTURE/langs \
RETRO_DECK_PYTHON=$FIXTURE/python \
  "$LAUNCHER" python
[ "$(cat "$FBTERM_ARGS_LOG")" = \
    "-n DejaVu Sans Mono -s 16 -f 7 -b 0 -- $FIXTURE/python" ] ||
  fail 'Python mode launches only the configured MicroPython interpreter'
[ "$(cat "$FBTERM_CWD_LOG")" = "$FIXTURE/langs/python" ] ||
  fail 'Python mode uses its persistent working directory'
[ "$(stat -c %a "$FIXTURE/langs/python")" = 700 ] ||
  fail 'Python working directory is private'

RETRO_DECK_TERMINAL_BASE=$FIXTURE \
RETRO_DECK_TERMINAL_TTY=$TEST_TTY \
RETRO_DECK_KEYMAP=us \
RETRO_DECK_LANG_BASE=$FIXTURE/langs \
RETRO_DECK_SCHEME=$FIXTURE/scheme \
RETRO_DECK_CHIBI_MODULE_PATH=$FIXTURE/chibi \
  "$LAUNCHER" scheme
[ "$(cat "$FBTERM_ARGS_LOG")" = \
    "-n DejaVu Sans Mono -s 16 -f 7 -b 0 -- $FIXTURE/scheme" ] ||
  fail 'Scheme mode launches only the configured Chibi interpreter'
[ "$(cat "$FBTERM_CWD_LOG")" = "$FIXTURE/langs/scheme" ] ||
  fail 'Scheme mode uses its persistent working directory'
[ "$(cat "$FBTERM_CHIBI_LOG")" = "$FIXTURE/chibi" ] ||
  fail 'Scheme mode exports the configured Chibi module library'
[ "$(stat -c %a "$FIXTURE/langs/scheme")" = 700 ] ||
  fail 'Scheme working directory is private'

: > "$LOADKEYS_LOG"
if RETRO_DECK_TERMINAL_BASE=$FIXTURE \
  RETRO_DECK_TERMINAL_TTY=$TEST_TTY \
  "$LAUNCHER" ruby >/dev/null 2>&1; then
  fail 'unsupported terminal program is rejected'
fi
[ ! -s "$LOADKEYS_LOG" ] ||
  fail 'unsupported terminal program does not change the console map'

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

command -v script >/dev/null 2>&1 ||
  fail 'script is required to verify console cleanup on a pseudo-terminal'
: > "$LOADKEYS_LOG"
CONSOLE_LOG=$FIXTURE/console.log
script -q -e -c \
  "env LOADKEYS_LOG=$LOADKEYS_LOG FBTERM_LOG=$FBTERM_LOG FBTERM_ARGS_LOG=$FBTERM_ARGS_LOG FBTERM_CWD_LOG=$FBTERM_CWD_LOG FBTERM_ECLDIR_LOG=$FBTERM_ECLDIR_LOG FBTERM_CHIBI_LOG=$FBTERM_CHIBI_LOG FBTERM_STDIN_LOG=$FBTERM_STDIN_LOG RETRO_DECK_TERMINAL_BASE=$FIXTURE RETRO_DECK_TERMINAL_TTY=/dev/tty RETRO_DECK_KEYMAP=us $LAUNCHER" \
  "$CONSOLE_LOG" >/dev/null
hide_cursor=$(printf '\033[?25l')
disable_blank=$(printf '\033[9;0]')
wake_console=$(printf '\033[13]')
show_cursor=$(printf '\033[?25h')
enable_blank=$(printf '\033[9;10]')
grep -Fq "$hide_cursor" "$CONSOLE_LOG" ||
  fail 'terminal exit hides the Linux console cursor'
grep -Fq "$disable_blank" "$CONSOLE_LOG" ||
  fail 'terminal exit disables Linux console blanking'
grep -Fq "$wake_console" "$CONSOLE_LOG" ||
  fail 'terminal exit wakes the Linux console'
if grep -Fq "$show_cursor" "$CONSOLE_LOG"; then
  fail 'terminal exit must not show the Linux console cursor'
fi
if grep -Fq "$enable_blank" "$CONSOLE_LOG"; then
  fail 'terminal exit must not re-enable Linux console blanking'
fi

printf '%s\n' 'retro-terminal-test: OK'
