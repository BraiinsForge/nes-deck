# Retro Deck

Retro Deck turns a Braiins Forge Deck into a persistent touch-first game and
program launcher. It boots directly into a native framebuffer dashboard with
NES, Game Boy, Game Boy Color, ZX Spectrum, CHIP-8, utilities, language REPLs,
and a chiptune player. Two Retro Games THEGamepad controllers work as stable
Player 1 and Player 2 devices.

## Deploy to a Deck

You need:

- a Braiins Forge Deck reachable as `root` over SSH
- a mounted `/mnt/data` partition on the Deck
- a Linux development machine with Nix flakes, SSH, SCP, tar, and gzip
- this private repository, including the owner-supplied `roms/` library
- stable power during activation

Clone the repository and run the deployment script with the Deck's address:

```sh
git clone git@github.com:BraiinsForge/nes-deck.git
cd nes-deck
./ops/deploy.sh root@10.0.0.10
```

The first build downloads the pinned ARM toolchain and can take several
minutes. The script builds every static runtime, verifies the staged payload,
uploads it below `/mnt/data`, briefly stops the dashboard, activates the new
files, and waits for `deck-menu` to be ready. If activation fails after the
menu is stopped, it attempts to restart the service before exiting.

The deployment script does not edit, reload, or disconnect Wi-Fi. It merges
the tracked ROMs and CC0 chiptunes into persistent storage without deleting
additional files, save games, or user programs already on the Deck.

Verify the result:

```sh
ssh root@10.0.0.10 '/etc/init.d/nes-deck status; \
  tail -n 40 /mnt/data/nes-deck/log/deck-menu.log'
```

To update an existing Deck, pull the repository and run the same deploy
command again.

If Nix is not installed, follow the
[official installation instructions](https://nixos.org/download/). Detailed
build and test commands are in [BUILD.md](BUILD.md).

## Included systems and programs

| Dashboard section | Runtime | Persistent content |
| --- | --- | --- |
| NES | FCEUmm | `/mnt/data/roms/nes` |
| Game Boy | Gambatte | `/mnt/data/roms/gb` |
| Game Boy Color | Gambatte | `/mnt/data/roms/gbc` |
| ZX Spectrum | Fuse | `/mnt/data/roms/zx` |
| CHIP-8 | c-octo | `/mnt/data/roms/chip8` |
| Deck | Native programs and REPLs | `/mnt/data/langs`, `/mnt/data/chiptunes` |

The Deck section contains:

- 10 Seconds, a native touch and controller timing game
- a `/bin/ash` framebuffer terminal with US ANSI and Czech QWERTZ layouts
- Lua 5.5, ECL Common Lisp 26.5.5, MicroPython 1.25, and Chibi Scheme 0.11
- a native GME and Ogg Vorbis chiptune player
- a guarded reboot action

REPL files persist under `/mnt/data/langs/{lua,lisp,python,scheme}`. The music
player scans `/mnt/data/chiptunes` for `ay`, `gbs`, `gym`, `hes`, `kss`, `nsf`,
`nsfe`, `ogg`, `sap`, `spc`, `vgm`, and `vgz` files. Ogg files must be 44.1 kHz
mono or stereo. Three CC0 tracks are included with provenance and checksums in
[chiptunes/README.md](chiptunes/README.md).

## Using the dashboard

Tap a system tab or use either controller's shoulder buttons to change
sections. Tap a visible card, or select it with Left/Right and press A, to
launch it. The small hollow rectangles show the selected position and total
number of entries.

The gear or controller Select opens settings. Its controls adjust volume and
backlight brightness, open the terminal, switch terminal keymaps, and add a
Wi-Fi profile. Volume and brightness persist below `/mnt/data`; volume uses
five-point steps and brightness is bounded from 10 through 100. Menu actions
play short cues while sound is enabled. The service disables console blanking
at boot and whenever a child program returns.

Hold the touchscreen for two seconds to leave a running emulator or terminal.
Touch does not emulate game controls. In the chiptune player, the top-right
cross returns immediately, the side arrows change files, the center button
pauses, `TRK -`/`TRK +` change subsongs, and the playback mode cycles through
loop all, loop one, and shuffle. Controller Left/Right changes files, Up/Down
changes the persistent volume in five-point steps, L/R changes subsongs, A
pauses, Start changes playback mode, and B returns.

The Wi-Fi editor only writes a root-only profile. Saving does not scan, roam,
reload networking, or disturb the current connection. The profile becomes
eligible when the current connection is later lost. Network and recovery
findings for the audited unit are in [DECK_NOTES.md](DECK_NOTES.md).

## Controllers

Identical THEGamepad devices are ordered by physical USB path. Keep them in the
same hub ports to preserve Player 1 and Player 2 across boots.

| Console control | THEGamepad |
| --- | --- |
| D-pad | D-pad |
| A | A or X |
| B | B or Y |
| Start | Start |
| Select | Back |

NES, GB, and GBC use this mapping. CHIP-8 uses the standard Octo mapping,
except Space Racer maps one controller to each ship. ZX Spectrum assigns
Kempston to Player 1 and Sinclair 2 to Player 2; A/X fires, Back opens the
Spectrum keyboard, L is Enter, and R is Space.

A keyboard remains a Player 1 fallback for NES:

| NES control | Keyboard |
| --- | --- |
| D-pad | Arrow keys or WASD |
| A | Z or J |
| B | X or K |
| Start | Enter |
| Select | Space or Shift |

## ROMs and save games

`roms/<system>/` is the canonical tracked library. Supported intake folders
are `nes`, `gb`, `gbc`, and `chip8`. A ROM or single-ROM ZIP at the
repository root is unprocessed intake and must be validated, renamed, filed,
checksummed, and added to the catalog before deployment. See
[roms/README.md](roms/README.md) for the exact intake contract.

The repository contains owner-supplied console ROMs and only freely licensed
CHIP-8 ROMs. The reproducible CHIP-8 sources, licenses, and hashes are recorded
in [FOSS_GAMES.md](FOSS_GAMES.md).

NES battery SRAM is saved atomically beside the ROM as `.srm`. GB and GBC use
`.sav` plus `.rtc` when the cartridge has a real-time clock. The deploy script
preserves these sidecars. ZX TAP files are read-only tape media and do not
produce automatic save files.

## Operations and recovery

Check the service and its bounded persistent log:

```sh
ssh root@10.0.0.10 '/etc/init.d/nes-deck status; \
  tail -n 100 /mnt/data/nes-deck/log/deck-menu.log'
```

Restart the dashboard without rebooting the Deck:

```sh
ssh root@10.0.0.10 '/etc/init.d/nes-deck restart'
```

If the display remains black, inspect the log before changing files or network
configuration. The launcher refuses to start before `/mnt/data` is mounted,
validates the framebuffer geometry, hides the console cursor, and unblanks the
panel on every return.

## Development

- [BUILD.md](BUILD.md) covers reproducible builds, tests, screenshots, and
  platform details.
- [deploy/menu/README.md](deploy/menu/README.md) defines the installed layout
  and strict catalog schema.
- [DECK_NOTES.md](DECK_NOTES.md) records verified hardware, audio, display,
  Wi-Fi, WireGuard, and recovery behavior.
- [AGENTS.md](AGENTS.md) defines repository-specific ROM handling.

## License

The project combines components under GPL, LGPL, BSD, MIT, and CC0 terms.
Exact upstream revisions are pinned, and required license texts are installed
with the binaries. Owner-supplied ROMs remain private and are not relicensed.
