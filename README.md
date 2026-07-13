# Retro Deck for Braiins Forge Deck

Play NES, Game Boy, Game Boy Color, CHIP-8, and Deck-native games on a Braiins Forge Deck
with a full-screen touch launcher. This project combines Deck-native
[FCEUmm](https://github.com/libretro/libretro-fceumm),
[Gambatte](https://github.com/libretro/gambatte-libretro), and
[c-octo](https://github.com/JohnEarnest/c-octo) frontends with a persistent
game catalog, a Wi-Fi profile editor, and an integrated framebuffer terminal.

## What You Need

- **Braiins Forge Deck**
- **USB-C PD Power Adapter**
- **USB-C Hub with PD Support**
- **USB Keyboard or another mapped input for gameplay**
- **ROM files** (`.nes`, `.gb`, `.gbc`, or `.ch8`)

The touchscreen operates the launcher. NES controls still use the keyboard
bindings below.

## Quick Start

### 1. Download Pre-compiled Binary

Download the latest release zip from the [releases page](https://github.com/BraiinsForge/deck-infones/releases/latest).

### 2. Access Your Deck via SSH

```bash
ssh root@<deck-ip>  # Use the admin password you set during setup
```

### 3. Stop the Deck Application

```bash
service bmc stop
```

### 4. Copy Files to Your Deck

```bash
unzip deck-infones.zip
scp nes-deck root@<deck-ip>:/tmp/
scp your-game.nes root@<deck-ip>:/tmp/
```

### 5. Run the Emulator

```bash
ssh root@<deck-ip>
chmod +x /tmp/nes-deck
/tmp/nes-deck /tmp/your-game.nes
```

The Deck port opens the active console keyboard itself. It also continues
without a keyboard, which is important for unattended boot.

## Touch launcher on boot

Deck firmware with a `/mnt/data` partition should keep the emulator and ROM
there instead of filling the small OpenWrt overlay. Build the six ARM
payloads and fetch the checksum-pinned homebrew set first:

```bash
nix build .#nes-deck -o result-nes-deck
nix build .#gb-deck -o result-gb-deck
nix build .#chip8-deck -o result-chip8-deck
nix build .#ten-seconds-deck -o result-ten-seconds
nix build .#deck-menu -o result-menu
nix build .#fbterm-deck -o result-fbterm
nix build -f nix/ecl-arm-static.nix -o result-ecl
./ops/deck-menu/fetch-foss-games.sh foss-games
```

The exact file map and catalog contract are documented in
[`deploy/menu/README.md`](deploy/menu/README.md). The installed service runs
`deck-menu` at S99, after `/mnt/data` is mounted:

```bash
ssh root@<deck-ip> 'mkdir -p /mnt/data/nes-deck/menu \
  /mnt/data/nes-deck/ecl /mnt/data/nes-deck/games \
  /mnt/data/nes-deck/licenses /mnt/data/nes-deck/terminal/fonts \
  /mnt/data/nes-deck/terminal/keymaps /mnt/data/roms/nes \
  /mnt/data/roms/gb /mnt/data/roms/gbc /mnt/data/roms/chip8'
scp result-nes-deck/bin/nes-deck root@<deck-ip>:/mnt/data/nes-deck/nes-deck
scp result-gb-deck/bin/gb-deck root@<deck-ip>:/mnt/data/nes-deck/gb-deck
scp result-chip8-deck/bin/chip8-deck root@<deck-ip>:/mnt/data/nes-deck/chip8-deck
scp result-ten-seconds/bin/ten-seconds-deck \
  root@<deck-ip>:/mnt/data/nes-deck/ten-seconds-deck
scp result-menu/bin/deck-menu root@<deck-ip>:/mnt/data/nes-deck/menu/deck-menu
scp result-fbterm/bin/fbterm root@<deck-ip>:/mnt/data/nes-deck/terminal/fbterm
scp result-fbterm/bin/loadkeys root@<deck-ip>:/mnt/data/nes-deck/terminal/loadkeys
scp result-fbterm/share/retro-deck/fonts/DejaVuSansMono.ttf \
  root@<deck-ip>:/mnt/data/nes-deck/terminal/fonts/
scp result-fbterm/share/retro-deck/keymaps/* \
  root@<deck-ip>:/mnt/data/nes-deck/terminal/keymaps/
scp -r result-fbterm/share/licenses/fbterm-deck \
  root@<deck-ip>:/mnt/data/nes-deck/licenses/
scp -r result-nes-deck/share/licenses/nes-deck \
  root@<deck-ip>:/mnt/data/nes-deck/licenses/
scp -r result-gb-deck/share/licenses/gb-deck \
  root@<deck-ip>:/mnt/data/nes-deck/licenses/
scp -r result-chip8-deck/share/licenses/chip8-deck \
  root@<deck-ip>:/mnt/data/nes-deck/licenses/
scp -r result-ecl/bin result-ecl/lib root@<deck-ip>:/mnt/data/nes-deck/ecl/
scp deploy/ecl root@<deck-ip>:/usr/bin/ecl
scp -r foss-games/roms/* root@<deck-ip>:/mnt/data/roms/
scp -r roms/nes roms/gb roms/gbc root@<deck-ip>:/mnt/data/roms/
scp foss-games/licenses/* root@<deck-ip>:/mnt/data/nes-deck/licenses/
scp deploy/menu/{games.sexp,games.tsv,compile-catalog.lisp,deck-menu-launcher} \
  root@<deck-ip>:/mnt/data/nes-deck/menu/
scp deploy/terminal/{retro-terminal,fonts.conf} \
  root@<deck-ip>:/mnt/data/nes-deck/terminal/
scp ops/deck-wifi/deck-wifi-profile-add \
  root@<deck-ip>:/usr/sbin/deck-wifi-profile-add
scp deploy/menu/nes-deck.init root@<deck-ip>:/etc/init.d/nes-deck.new

ssh root@<deck-ip> '
  chmod 755 /mnt/data/nes-deck/nes-deck /mnt/data/nes-deck/gb-deck \
    /mnt/data/nes-deck/chip8-deck /mnt/data/nes-deck/ten-seconds-deck \
    /mnt/data/nes-deck/menu/deck-menu \
    /mnt/data/nes-deck/menu/deck-menu-launcher \
    /mnt/data/nes-deck/terminal/fbterm \
    /mnt/data/nes-deck/terminal/loadkeys \
    /mnt/data/nes-deck/terminal/retro-terminal \
    /usr/sbin/deck-wifi-profile-add /usr/bin/ecl \
    /etc/init.d/nes-deck.new
  /etc/init.d/bmc stop
  /etc/init.d/bmc disable
  /etc/init.d/nes-deck stop
  mv /etc/init.d/nes-deck.new /etc/init.d/nes-deck
  /etc/init.d/nes-deck enable
  /etc/init.d/nes-deck start
'
```

Use the **NES**, **GAME BOY**, **GAME BOY COLOR**, **CHIP-8**, and **DECK** tabs to
filter the title-only game cards, then tap a card to start it. The selected
tab is highlighted and cards retain stable catalog-to-launch mappings after
filtering. Every game name uses the same compact font size. The **- / VOL / +**
control changes the next game's PCM volume in
5-point steps from 0 through 100. Tap the green volume display to mute it; the
display turns red and reads **VOL OFF**. Tap that display or **+** to restore
the last audible level, or the configured default if the launcher started
muted. Each nonzero adjustment plays a short two-note confirmation at the
selected level. The value is saved under `/mnt/data` and survives reboot. While
a game is running, hold anywhere on the touchscreen for two seconds to return
to the menu. Touch does not emulate a game controller. The emulators support
two Retro Games
THEGamepad USB controllers (`1c59:0026`) with stable player ordering, while
the keyboard remains a Player 1 fallback. Space Racer uses both controllers
simultaneously.

Battery-backed cartridge saves are automatic and live beside their ROMs.
The FCEUmm frontend writes changed NES SRAM atomically to `.srm` every ten
seconds and on exit. It migrates the compressed `.srm` format written by the
earlier InfoNES frontend on first load. The GB/GBC frontend does the same with
`.sav` and, when the cartridge has a real-time clock, `.rtc`. Games without
battery-backed storage do not create save sidecars; this is cartridge saving,
not arbitrary emulator save states.

The computer icon opens a real framebuffer shell with a 16-pixel safe area for
the display's rounded corners. **KEYS US** and **KEYS CZ** select the terminal
layout; the launcher applies it only for that terminal session and restores US
ANSI afterward. Type `exit` or use the same two-second touch hold to return.
The **WIFI** action opens a touch keyboard for adding PSK
networks. Saving a network never reloads or changes the live Wi-Fi connection.
A profile with the same SSID atomically replaces all older records and becomes
eligible only after the current connection is lost.

The **10 SECONDS** Deck game starts and stops on touch press.

The launcher is supervised by procd and shuts the emulator down cleanly so
keyboard mode is restored. The bounded persistent log includes catalog,
terminal, and emulator exit details. Check it with:

```bash
ssh root@<deck-ip> '/etc/init.d/nes-deck status; \
  tail -n 100 /mnt/data/nes-deck/log/deck-menu.log'
```

The freely licensed catalog contains Outlaw and the simultaneous two-player
Space Racer for CHIP-8. Their pinned sources, license, and hashes are in
[FOSS_GAMES.md](FOSS_GAMES.md). The private repository retains the
owner-supplied library under `roms/<system>/` so another clone can reproduce
the working collection. A ROM or single-ROM ZIP at the repository root is an
unprocessed intake file; the filing contract is documented in
[`roms/README.md`](roms/README.md).

Hardware, framebuffer, audio, Wi-Fi, WireGuard, and recovery findings for the
audited Deck are kept in [DECK_NOTES.md](DECK_NOTES.md). Wi-Fi selector source
is under [`ops/deck-wifi`](ops/deck-wifi/); it contains no credentials.

## Controls

Identical gamepads are assigned by physical USB path, so keeping them in the
same hub ports keeps Player 1 and Player 2 stable. The emulator logs the path
assigned to each player at launch. For a temporary hardware audit, launch with
`INFONES_INPUT_DIAGNOSTICS=1` to log only controller state changes.

| NES Button | THEGamepad |
|------------|------------|
| D-Pad | D-Pad |
| A | A or X |
| B | B or Y |
| Start | Start |
| Select | Back |

GB/GBC use the same D-pad, A, B, Start, and Select mapping. CHIP-8's standard
Octo profile maps the D-pad to WASD, A/X to E, B/Y to Q, Back to Z, and Start
to V. Space Racer instead maps controller 1 up/down to the left ship and
controller 2 up/down to the right ship.

Keyboard controls apply to Player 1:

| NES Button | Keyboard |
|------------|----------|
| D-Pad | Arrow keys or WASD |
| A | Z or J |
| B | X or K |
| Start | Enter |
| Select | Space or Shift |

## Getting more ROMs

To extend the private library, you can:

1. Run the pinned FOSS game fetcher described above
2. Use other clearly licensed homebrew ROMs for a supported system
3. Dump your own cartridges

## Building from Source

Build instructions for every emulator are in [BUILD.md](BUILD.md).

## License

FCEUmm and Gambatte are GPL-2.0-only. The vendored fbterm source is GPL-2; its
license and exact upstream provenance are retained under
[`terminal/`](terminal/). c-octo is MIT licensed. Exact upstream revisions are
pinned in `flake.lock`, and license texts are installed with the binaries.
