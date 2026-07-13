# Deck menu deployment bundle

This directory contains the catalog and boot plumbing for the persistent
touchscreen menu.  It does not contain ROMs, the emulator, ECL, or the native
`deck-menu` binary.

## Installed layout

Copy these files to the Deck without changing their basenames:

| Repository file | Deck path |
| --- | --- |
| `games.sexp` | `/mnt/data/nes-deck/menu/games.sexp` |
| `games.tsv` | `/mnt/data/nes-deck/menu/games.tsv` |
| `compile-catalog.lisp` | `/mnt/data/nes-deck/menu/compile-catalog.lisp` |
| `deck-menu-launcher` | `/mnt/data/nes-deck/menu/deck-menu-launcher` |
| `nes-deck.init` | `/etc/init.d/nes-deck` |

The launcher also expects:

- `/mnt/data/nes-deck/menu/deck-menu`
- `/mnt/data/nes-deck/infones`
- `/mnt/data/nes-deck/ecl/bin/ecl.bin` (ECL 26.5.5)
- `/mnt/data/nes-deck/ecl/lib/ecl/` (the ECL runtime directory)
- the ROM paths listed in `games.sexp`

The launcher exports the exact trailing-slash runtime path
`ECLDIR=/mnt/data/nes-deck/ecl/lib/ecl/`.  It initializes persistent sound
state at `/mnt/data/nes-deck/state/menu-sound.state` to `on`; enabled emulator
volume is 42 percent through `INFONES_VOLUME_PERCENT=42`.  The generated
manifest and sound state stay under `/mnt/data/nes-deck/state`.  A bounded
persistent launcher log is kept at `/mnt/data/nes-deck/log/deck-menu.log`, and
procd also sends standard output and errors to logd.

At runtime, tap a game card to launch it. The top-right sound button writes the
canonical state atomically; ON passes the launcher's enabled volume to InfoNES
and OFF passes zero. Switching from OFF to ON plays a short two-note
confirmation chime. A continuous two-second hold anywhere on the touchscreen
terminates the emulator child and redraws the menu. Touch does not supply NES
controller input; a keyboard or mapped controller is still needed to press
Start and play.

## Catalog contract

`games.sexp` contains one schema-versioned property list.  Each game has these
six required keys:

1. `:id` - lowercase stable identifier
2. `:title` - menu title
3. `:rom` - normalized absolute `.nes` path under `/mnt/data/nes-deck/`
4. `:description` - short menu copy
5. `:color` - `#RRGGBB` accent color
6. `:license` - concise redistribution/license label

`compile-catalog.lisp` permits no missing, duplicate, or unknown keys.  It
rejects duplicate IDs and ROM paths, dotted/circular/oversized forms, reader
evaluation, control characters, non-ASCII text, malformed colors, and paths
outside the persistent installation.  The output is headerless TSV in the
field order above.  It is written beside a process-specific temporary file and
atomically renamed only after the complete catalog validates.

The checked-in `games.tsv` is a known-good fallback.  If ECL, the source
catalog, or generation is unavailable, the launcher uses that file and logs
the reason.  No shell evaluates catalog content.

## Pre-deployment check

With ECL available on the build machine:

```sh
ecl --norc --shell compile-catalog.lisp games.sexp /tmp/games.tsv
cmp games.tsv /tmp/games.tsv
```

At deployment, make the launcher, menu, emulator, and init script executable.
The init script is intentionally installed under the existing service name so
it replaces the old direct-ROM S99 launcher rather than racing it.
