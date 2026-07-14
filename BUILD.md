# Build and test Retro Deck

Retro Deck uses Nix for reproducible ARMv7 hard-float builds. The generated
executables are static and run on the Deck's OpenWrt userspace without copying
a Nix store closure to the device.

For ordinary installation, use the complete deployment command in
[README.md](README.md):

```sh
./ops/deploy.sh root@10.0.0.10
```

This document covers individual builds, verification, tests, and platform
details for development.

## Prerequisites

Install Nix with flakes enabled. Host tests also require a C and C++ compiler,
`pkg-config`, and libpng development headers.

On Debian or Ubuntu:

```sh
sudo apt-get install build-essential pkg-config libpng-dev
```

Then clone the private repository:

```sh
git clone git@github.com:BraiinsForge/nes-deck.git
cd nes-deck
```

The first Nix build downloads the pinned cross toolchain and may take several
minutes. Later builds reuse the Nix store.

## Build packages individually

Use `--no-link` to avoid leaving `result-*` symlinks in the repository:

```sh
nix build --no-link --print-out-paths .#nes-deck
nix build --no-link --print-out-paths .#gb-deck
nix build --no-link --print-out-paths .#zx-deck
nix build --no-link --print-out-paths .#chip8-deck
nix build --no-link --print-out-paths .#ten-seconds-deck
nix build --no-link --print-out-paths .#deck-menu
nix build --no-link --print-out-paths .#fbterm-deck
nix build --no-link --print-out-paths .#lua-deck
nix build --no-link --print-out-paths .#python-deck
nix build --no-link --print-out-paths .#chibi-deck
nix build --no-link --print-out-paths .#chiptune-deck
nix build --no-link --print-out-paths -f nix/ecl-arm-static.nix
```

| Package | Main output |
| --- | --- |
| `nes-deck` | `bin/nes-deck` |
| `gb-deck` | `bin/gb-deck` |
| `zx-deck` | `bin/zx-deck` |
| `chip8-deck` | `bin/chip8-deck` |
| `ten-seconds-deck` | `bin/ten-seconds-deck` |
| `deck-menu` | `bin/deck-menu` |
| `fbterm-deck` | `bin/{fbterm,loadkeys}` plus font and keymaps |
| `lua-deck` | `bin/lua` |
| `python-deck` | `bin/python` |
| `chibi-deck` | `bin/chibi-scheme` plus Scheme modules |
| `chiptune-deck` | `bin/chiptune-deck` |
| ECL expression | `bin/ecl.bin` plus the ECL runtime library |

Check that a package has no Nix runtime references before deploying it:

```sh
out=$(nix build --no-link --print-out-paths .#chiptune-deck | tail -n 1)
file "$out/bin/chiptune-deck"
test -z "$(nix-store -q --references "$out")"
```

The expected executable is a statically linked 32-bit ARM EABI5 binary.

## Run the host test suite

The test runner compiles into a temporary directory and leaves the worktree
clean:

```sh
tests/run-host-tests.sh
```

It covers the NES mixer, APU noise, SRAM codec, two-controller ordering,
dashboard geometry and behavior, ROM catalog, cover cache, Wi-Fi profile
helper, terminal lifecycle, shared framebuffer/audio runtime, timer
configuration, and CHIP-8 core.

Run shell checks on deployment code with:

```sh
nix shell nixpkgs#shellcheck -c shellcheck \
  ops/deploy.sh tests/run-host-tests.sh
```

## Validate language and music runtimes on a Deck

The deploy script performs basic Python, Scheme, and dashboard smoke tests
before stopping the running service. For focused checks against a staged
binary:

```sh
/mnt/data/nes-deck/langs/python -c 'print(6 * 7)'
CHIBI_MODULE_PATH=/mnt/data/nes-deck/langs/chibi/lib \
  /mnt/data/nes-deck/langs/chibi/chibi-scheme -q -p '(+ 20 22)'
/mnt/data/nes-deck/chiptune-deck --probe \
  /mnt/data/chiptunes/crazy.ogg
```

The chiptune player can render its UI without opening the framebuffer:

```sh
/mnt/data/nes-deck/chiptune-deck --render-preview \
  /mnt/data/chiptunes/crazy.ogg /tmp/chiptune-player.ppm
```

## Render dashboard screenshots

Copy the persistent cover cache from a Deck, then run the native renderer:

```sh
scp -r root@10.0.0.10:/mnt/data/nes-deck/covers /tmp/deck-covers
ops/deck-menu/render-screenshots.sh deploy/menu/games.tsv \
  /tmp/deck-covers "$HOME/retro-deck-screens"
```

The output contains every game selection, settings variants, the Wi-Fi
keyboard, reboot confirmation, timer, and a contact sheet.

## Platform details

The Deck CPU is ARMv7 Cortex-A7 hard-float. Its panel is a portrait 600x1280
RGB565 framebuffer used as a 1280x480 logical landscape display. The physical
pitch is 1280 bytes, including 80 bytes of padding per row, and only physical
columns 0 through 479 are visible. Code must use the stride reported by
`FBIOGET_FSCREENINFO`, not `xres * bytes_per_pixel`.

The menu fills the complete 1280x480 logical surface. Emulators and the
chiptune player use the shared scaler with a 16-pixel safe inset for the
rounded display. fbterm uses a 1248x448 viewport for the same reason. Every
frontend rejects unexpected geometry or color channel layouts rather than
guessing.

Audio uses `/dev/dsp` through the Deck's ALSA OSS bridge. The hardware stream
is S16_LE stereo. NES, ZX, CHIP-8, the timer, menu cues, and chiptunes use
44.1 kHz. Gambatte produces 32768 Hz and is explicitly resampled to the Deck's
verified 32000 Hz OSS rate. Gain is applied in the native mixer because the
kernel OSS path bypasses ALSA userspace soft volume.

The framebuffer has no page-flip API. Frontends build complete frames in
cacheable memory and copy finished rows to fb0 to reduce tearing and protect
audio timing. `INFONES_RUNTIME_DIAGNOSTICS=1` logs 120-frame timing windows.
`INFONES_VSYNC=1` remains an experimental opt-in because the audited driver can
stall sound and frame pacing.

## Source layout

```text
nes-deck/
├── chiptunes/                  CC0 seed tracks and provenance
├── deploy/
│   ├── menu/                   catalog, launcher, and procd service
│   └── terminal/               fbterm wrapper, fontconfig, and keymaps
├── nix/                        ECL and runtime-specific Nix expressions
├── ops/
│   ├── deck-menu/              covers, screenshots, and FOSS CHIP-8 fetcher
│   ├── deck-wifi/              profile-only Wi-Fi helper
│   └── deploy.sh               complete staged deployment
├── patches/                    pinned upstream fixes
├── roms/                       private canonical ROM library and checksums
├── src/
│   ├── deck_menu.cpp           dashboard, settings, and child supervision
│   ├── deck_runtime.cpp        framebuffer, audio, and frame clock
│   ├── libretro_deck.cpp       NES, GB/GBC, and ZX host
│   ├── chip8_deck.cpp          CHIP-8 frontend
│   ├── chiptune_deck.cpp       GME and Ogg native music player
│   ├── ten_seconds_deck.cpp    native timing game
│   └── joypad_input.cpp        stable two-controller input
├── terminal/                   vendored fbterm source and provenance
├── tests/                      host regression suite
├── flake.nix                   pinned cross-build definitions
└── README.md                   deployment and operation guide
```

The exact on-device file contract and strict catalog schema are documented in
[deploy/menu/README.md](deploy/menu/README.md).
