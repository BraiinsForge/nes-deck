# NES Emulator for Braiins Forge Deck

Play NES games on a Braiins Forge Deck with a full-screen touch launcher. This
project combines a Deck-specific [InfoNES](https://github.com/nejidev/arm-NES-linux)
port, a persistent game catalog, and a small native boot menu.

## What You Need

- **Braiins Forge Deck**
- **USB-C PD Power Adapter**
- **USB-C Hub with PD Support**
- **USB Keyboard or another mapped input for gameplay**
- **NES ROM files** (.nes format)

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
scp infones root@<deck-ip>:/tmp/
scp your-game.nes root@<deck-ip>:/tmp/
```

### 5. Run the Emulator

```bash
ssh root@<deck-ip>
chmod +x /tmp/infones
/tmp/infones /tmp/your-game.nes
```

The Deck port opens the active console keyboard itself. It also continues
without a keyboard, which is important for unattended boot.

## Touch launcher on boot

Deck firmware with a `/mnt/data` partition should keep the emulator and ROM
there instead of filling the small OpenWrt overlay. Build the three ARM
payloads and fetch the checksum-pinned homebrew set first:

```bash
nix build .#infones-deck -o result-infones
nix build .#deck-menu -o result-menu
nix build -f nix/ecl-arm-static.nix -o result-ecl
./ops/deck-menu/fetch-foss-games.sh foss-games
```

The exact file map and catalog contract are documented in
[`deploy/menu/README.md`](deploy/menu/README.md). The installed service runs
`deck-menu` at S99, after `/mnt/data` is mounted:

```bash
ssh root@<deck-ip> 'mkdir -p /mnt/data/nes-deck/menu \
  /mnt/data/nes-deck/ecl /mnt/data/nes-deck/roms \
  /mnt/data/nes-deck/licenses'
scp result-infones/bin/infones root@<deck-ip>:/mnt/data/nes-deck/infones
scp result-menu/bin/deck-menu root@<deck-ip>:/mnt/data/nes-deck/menu/deck-menu
scp -r result-ecl/bin result-ecl/lib root@<deck-ip>:/mnt/data/nes-deck/ecl/
scp deploy/ecl root@<deck-ip>:/usr/bin/ecl
scp foss-games/roms/* root@<deck-ip>:/mnt/data/nes-deck/roms/
scp foss-games/licenses/* root@<deck-ip>:/mnt/data/nes-deck/licenses/
scp deploy/menu/{games.sexp,games.tsv,compile-catalog.lisp,deck-menu-launcher} \
  root@<deck-ip>:/mnt/data/nes-deck/menu/
scp deploy/menu/nes-deck.init root@<deck-ip>:/etc/init.d/nes-deck.new

ssh root@<deck-ip> '
  chmod 755 /mnt/data/nes-deck/infones /mnt/data/nes-deck/menu/deck-menu \
    /mnt/data/nes-deck/menu/deck-menu-launcher /usr/bin/ecl \
    /etc/init.d/nes-deck.new
  /etc/init.d/bmc stop
  /etc/init.d/bmc disable
  /etc/init.d/nes-deck stop
  mv /etc/init.d/nes-deck.new /etc/init.d/nes-deck
  /etc/init.d/nes-deck enable
  /etc/init.d/nes-deck start
'
```

Tap a game card to start it. The top-right **SOUND ON/OFF** button controls
whether the next game starts at the known-good 42% PCM volume or fully muted;
switching from OFF to ON plays a short two-note confirmation chime. The choice
is saved under `/mnt/data` and survives reboot. While a game is running, hold
anywhere on the touchscreen for two seconds to return to the menu. Touch does
not emulate an NES controller, so attach a keyboard or mapped controller for
Start and gameplay.

The launcher is supervised by procd and shuts the emulator down cleanly so
keyboard mode is restored. Check it with:

```bash
ssh root@<deck-ip> '/etc/init.d/nes-deck status; logread -e nes-deck-menu'
```

The FOSS catalog currently contains Falling, Thwaite, Concentration Room, and
robotfindskitten. Their pinned sources, licenses, mapper details, and hashes
are in [FOSS_GAMES.md](FOSS_GAMES.md). A locally supplied `mario.nes` can also
remain in the menu, but this repository does not distribute it.

Hardware, framebuffer, audio, Wi-Fi, WireGuard, and recovery findings for the
audited Deck are kept in [DECK_NOTES.md](DECK_NOTES.md). Wi-Fi selector source
is under [`ops/deck-wifi`](ops/deck-wifi/); it contains no credentials.

## Controls

| NES Button | Keyboard |
|------------|----------|
| D-Pad | Arrow keys or WASD |
| A | Z or J |
| B | X or K |
| Start | Enter |
| Select | Space or Shift |

## Getting more NES ROMs

ROM binaries are not committed to this repository. You can:

1. Run the pinned FOSS game fetcher described above
2. Use other homebrew ROMs from [NESdev](https://www.nesdev.org/)
3. Dump your own cartridges

## Building from Source

Want to compile InfoNES yourself instead of using the pre-built binary? Check out [BUILD.md](BUILD.md) for complete instructions.

## License

InfoNES is freeware for non-commercial use. See the original InfoNES documentation for details.
