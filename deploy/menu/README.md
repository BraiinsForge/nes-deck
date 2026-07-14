# Deck menu deployment bundle

This directory contains the catalog and boot plumbing for the persistent
touchscreen menu. It does not contain ROMs, emulators, language runtimes, or
the native `deck-menu` binary.

## Installed layout

Copy these files to the Deck without changing their basenames:

| Repository file | Deck path |
| --- | --- |
| `games.sexp` | `/mnt/data/nes-deck/menu/games.sexp` |
| `games.tsv` | `/mnt/data/nes-deck/menu/games.tsv` |
| `compile-catalog.lisp` | `/mnt/data/nes-deck/menu/compile-catalog.lisp` |
| `deck-menu-launcher` | `/mnt/data/nes-deck/menu/deck-menu-launcher` |
| `nes-deck.init` | `/etc/init.d/nes-deck` |

The separate uploader bundle installs `nes-deck-uploader.init` as
`/etc/init.d/nes-deck-uploader` and the static service as
`/mnt/data/nes-deck/uploader/rom-uploader`.

The launcher also expects:

- `/mnt/data/nes-deck/menu/deck-menu`
- `/mnt/data/nes-deck/nes-deck`
- `/mnt/data/nes-deck/gb-deck`
- `/mnt/data/nes-deck/zx-deck`
- `/mnt/data/nes-deck/chip8-deck`
- `/mnt/data/nes-deck/ten-seconds-deck`
- `/mnt/data/nes-deck/chiptune-deck`
- `/mnt/data/nes-deck/ecl/bin/ecl.bin` (ECL 26.5.5)
- `/mnt/data/nes-deck/ecl/lib/ecl/` (the ECL runtime directory)
- `/mnt/data/nes-deck/langs/lua` (Lua 5.5.0)
- `/mnt/data/nes-deck/langs/python` (MicroPython 1.25)
- `/mnt/data/nes-deck/langs/chibi/chibi-scheme` (Chibi Scheme 0.11)
- `/mnt/data/nes-deck/langs/chibi/lib/` (Chibi Scheme module library)
- `/mnt/data/nes-deck/terminal/retro-terminal`
- `/mnt/data/nes-deck/terminal/{fbterm,loadkeys,rlwrap,keymaps/}`
- `/usr/sbin/deck-wifi-profile-add`
- `/mnt/data/roms/{nes,gb,gbc,zx,chip8}/` and the ROM paths listed in
  `games.sexp`
- `/mnt/data/langs/{lua,lisp,python,scheme}/` for persistent REPL files
- `/mnt/data/chiptunes/` for user and tracked music files
- `/mnt/data/nes-deck/uploads/games.tsv` for validated web uploads

The launcher exports the exact trailing-slash runtime path
`ECLDIR=/mnt/data/nes-deck/ecl/lib/ecl/`. It initializes persistent volume at
`/mnt/data/nes-deck/state/menu-volume.state` to 42, adopts the current display
backlight in `/mnt/data/nes-deck/state/menu-brightness.state`, and initializes
terminal layout at `/mnt/data/nes-deck/state/terminal-keymap.state` to `us`.
Brightness is persisted in 10-point steps from 10 through 100 so the dashboard
cannot turn the panel fully black. A legacy exact `on`/`off` sound state
migrates to 42/0. The generated manifest and persistent control state stay
under `/mnt/data/nes-deck/state`. A bounded
persistent launcher log is kept at `/mnt/data/nes-deck/log/deck-menu.log`.
The native menu appends child start, exit-status, and signal details there;
launcher milestones are also sent to logd.
The launcher disables the Linux console's ten-minute blank timer at each boot,
and the native menu explicitly unblanks fb0 whenever it reopens the display.
Every managed child return also hides the Linux console cursor and keeps
console blanking disabled, including after the framebuffer terminal exits.
When the uploader catalog is present, the launcher combines it with the
generated repository catalog and asks `deck-menu --validate-manifest` to
validate the complete file and every ROM before using it. An invalid upload
catalog is logged and ignored rather than preventing the dashboard from
starting.

At boot, `fetch-covers` fills the persistent cover cache once per game. It
prefers Libretro box art, then falls back to a title screen and finally a
gameplay snapshot when a system's box-art set is incomplete. Cached images,
source URLs, system indexes, and confirmed misses are reused on later boots.

At runtime, every populated console is a top tab and the carousel shows at most
three games. Tap a tab or use a THEGamepad controller's L/R shoulders to switch
consoles. Left/Right moves the selected game, A launches it, and the hollow
marker row preserves the complete game count. Successful controller and
touchscreen navigation plays a short directional, enter, or back chiptune while
volume is audible. An isolated sound worker keeps input responsive, and input
arriving during a cue is discarded. Each game retains its original catalog
index for launch routing. Descriptions and license labels stay out of the
launcher; redistribution and license details remain in `FOSS_GAMES.md` and the
installed license files.

USB keyboards are discovered and hot-plugged through evdev. Arrows move the
selection, Enter activates it, Escape goes back, Tab moves to the next console,
and Shift-Tab moves to the previous console. The dashboard exclusively grabs
each keyboard while visible, then releases it before a managed child starts.

The top-right gear or controller Select opens settings. D-pad directions move
among volume down/up, brightness down/up, terminal, keymap, and Wi-Fi; A
activates the selected control and B or the cross closes the screen. Volume is
atomically persisted in 5-point steps from 0 through 100. Plus while muted
restores the last audible level, or the configured default if the launcher
started muted. Every nonzero adjustment plays a short confirmation chime. The
selected volume is passed to every emulator. Brightness updates
`/sys/class/backlight/display-bl/brightness` and persists in safe 10-point steps
without changing any network state. A continuous two-second hold anywhere on
the touchscreen
terminates the emulator child and redraws the menu. Touch does not supply
controller input; a keyboard or mapped controller is still needed to press
Start and play.

The Deck-native **10 SECONDS** game owns touch while it runs and has its own
BACK action. Physical A on either controller also starts and stops it. Short
start and result chiptunes follow the dashboard volume.

The settings computer icon and DECK terminal entry launch
`/mnt/data/nes-deck/terminal/retro-terminal`; the control subtitle identifies
the fixed `/bin/ash` login shell. The adjacent keymap action toggles between US
ANSI and Czech QWERTZ. The terminal launcher applies that map for fbterm and
restores US when its child exits or the menu terminates it. The DECK carousel
also routes exact `lua`, `lisp`, `python`, and `scheme` modes to Lua 5.5.0,
ECL 26.5.5, MicroPython 1.25, and Chibi Scheme 0.11. They start in private
persistent working directories below `/mnt/data/langs`; no catalog or user
text is evaluated as a command. ECL runs through the static `rlwrap` payload,
with private history at `/mnt/data/langs/lisp/.ecl_history`.

The built-in CHIPTUNES entry runs the native player against
`/mnt/data/chiptunes`. It supports the GME console-music formats plus 44.1 kHz
mono or stereo Ogg Vorbis. Its bottom controls select playback mode, previous
file, play/pause, and next file; the top-right cross closes the player.
Controller Up/Down changes and persists volume, controller L/R selects
subsongs, and Start changes playback mode. Files are read with a 16 MiB limit,
directory recursion is
bounded, and symbolic links are not followed.

The Deck carousel adds a built-in red power-on entry for `/sbin/reboot`; two
selections within four seconds are required, and any other action cancels the
armed request. The WIFI button opens the on-screen keyboard and passes
credentials to `deck-wifi-profile-add` over stdin, never argv. The helper only writes a
root-only profile; it does not scan, reload, roam, or alter the active network.
Saving an existing SSID commits the canonical replacement first and then
removes duplicate plain/hex profile names.

## Catalog contract

`games.sexp` contains one schema-versioned property list. Each game has these
five required keys:

1. `:id` - lowercase stable identifier
2. `:title` - menu title
3. `:system` - one of `:nes`, `:gb`, `:gbc`, `:zx`, `:chip8`, or `:deck`
4. `:rom` - normalized absolute path below `/mnt/data/roms/<system>/` with the
   system's required extension; Deck applications stay below
   `/mnt/data/nes-deck/games/`
5. `:color` - exact canonical xterm-256 `#RRGGBB` accent color

`compile-catalog.lisp` permits no missing, duplicate, or unknown keys. It
rejects duplicate IDs and ROM paths, dotted/circular/oversized forms, reader
evaluation, control characters, non-ASCII text, malformed colors, and paths
outside the persistent installation. It also rejects colors outside the fixed
xterm-256 palette. The output is headerless TSV in the field order above. It is
written beside a process-specific temporary file and
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
