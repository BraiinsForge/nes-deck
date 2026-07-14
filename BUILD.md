# Building Retro Deck emulators for Braiins Forge Deck

This guide explains how to build the NES, GB/GBC, ZX Spectrum, and CHIP-8
emulators, the
Deck-native ten-second game, and
launcher for the Braiins Forge Deck.

## Prerequisites

The Deck uses an ARMv7 Cortex-A7 (`arm_cortex-a7_neon-vfpv4`) hard-float
processor and OpenWrt uses musl. The provided build emits a static ARMv7
hard-float binary, so it does not depend on the Deck's userspace C library.

## Option 1: Using Nix (Recommended)

Nix is a reproducible build system and package manager that ensures consistent builds across different machines. The easiest way to build InfoNES is using the provided Nix flake, which sets up the complete cross-compilation environment automatically.

Alternatively, you can build by hand if you procure an ARM cross-compilation toolchain.

### Installing Nix

If you don't have Nix installed, install it with flakes support:

```bash
curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install
```

For more information, see the [Nix installation guide](https://nixos.org/download.html).

### Quick Build (Flake)

The fastest way to build is using the Nix flake directly, without cloning the repository:

```bash
nix build github:BraiinsForge/deck-infones
```

The binary will be at `./result/bin/infones`.

### Advanced Development Workflow

For active development and iteration, use the Nix development environment:

1. Clone this repository:
   ```bash
   git clone git@github.com:BraiinsForge/deck-infones.git
   cd deck-infones
   ```

2. Enter the Nix development environment:
   ```bash
   nix develop
   ```

   **Note:** The first run downloads the ARM toolchain and builds dependencies, which can take several minutes.

3. Build using Nix:
   ```bash
   nix build
   ```

   The compiled binary will be at `./result/bin/infones`.

   Build the static ARM touch launcher and language runtimes separately:
   ```bash
   nix build .#deck-menu -o result-menu
   nix build .#gb-deck -o result-gb-deck
   nix build .#zx-deck -o result-zx-deck
   nix build .#chip8-deck -o result-chip8-deck
   nix build .#ten-seconds-deck -o result-ten-seconds
   nix build .#fbterm-deck -o result-fbterm
   nix build .#lua-deck -o result-lua
   nix build -f nix/ecl-arm-static.nix -o result-ecl
   ```

   Those payloads include `result-menu/bin/deck-menu`,
   `result-lua/bin/lua`, and `result-ecl/{bin/ecl.bin,lib/ecl/help.doc}`.
   Lua 5.5.0 is pinned to its official source archive, and both language
   interpreters are closure-free static ARM binaries.

4. Run the host-side audio checks after mixer changes:
   ```bash
   g++ -std=c++11 -Wall -Wextra -Werror -Isrc \
     tests/nes_audio_test.cpp -o nes_audio_test
   ./nes_audio_test

   g++ -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
     tests/joypad_input_test.cpp -pthread -o joypad-input-test
   ./joypad-input-test

   g++ -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
     src/deck_menu.cpp $(pkg-config --cflags --libs libpng) \
     -o deck-menu-host
   ./deck-menu-host --geometry-test
   g++ -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
     tests/deck_menu_test.cpp $(pkg-config --cflags --libs libpng) \
     -o deck-menu-test
   ./deck-menu-test
   tests/deck_wifi_profile_add_test.sh
   tests/retro_terminal_test.sh

   g++ -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
     tests/deck_runtime_test.cpp src/deck_runtime.cpp -pthread \
     -o deck-runtime-test
   ./deck-runtime-test

   octo_src=$(nix eval --raw --impure --expr \
     '(builtins.getFlake ("path:" + toString ./.)).inputs."c-octo-src".outPath')
   cc -std=c99 -O2 -Wall -Wextra -Werror -I"$octo_src/src" \
     tests/chip8_core_test.c src/chip8_core.c -o chip8-core-test
   ./chip8-core-test
   ```

5. Render a designer-facing dashboard screenshot set from the checked-in
   catalog and the Deck's persistent cover cache:
   ```bash
   scp -r root@10.0.0.10:/mnt/data/nes-deck/covers /tmp/deck-covers
   ops/deck-menu/render-screenshots.sh deploy/menu/games.tsv \
     /tmp/deck-covers "$HOME/retro-deck-screens"
   ```
   The renderer calls the native menu drawing code directly, emits every
   console and game-carousel state at 1280x480, includes control and Wi-Fi
   variants, and builds `00-overview.png` as a contact sheet.

The GB/GBC package builds the GPL-2.0-only Gambatte libretro core at the exact
revision in `flake.lock`, using Cortex-A7/NEON flags, LTO, and native RGB565
output. The ZX package statically links the GPL-3.0 Fuse libretro core pinned
in the same lock file, enables fast TAP autoload, uses the 288x216 medium-border
frame for exact 2x scaling, and assigns Kempston and Sinclair 2 joysticks to
the two stable controller ports. The CHIP-8 package
statically links the pinned MIT c-octo core through `src/chip8_core.c`. They use
the shared framebuffer/audio implementation in `src/deck_runtime.cpp` and the
same stable two-controller discovery as InfoNES.

## Option 2: Manual Toolchain Setup

If you prefer not to use Nix, you can manually install an ARM cross-compiler.

### Install ARM Toolchain

**Debian/Ubuntu:**
```bash
sudo apt install gcc-arm-linux-gnueabihf g++-arm-linux-gnueabihf
```

**Arch Linux:**
```bash
yay -S arm-linux-gnueabihf-gcc
```

### Clone Repository

```bash
git clone git@github.com:BraiinsForge/deck-infones.git
cd deck-infones
```

### Build

This project patches the upstream InfoNES source at build time. To build manually, you'll need to:

1. Clone the upstream InfoNES:
   ```bash
   git clone https://github.com/nejidev/arm-NES-linux.git infones-upstream
   git -C infones-upstream checkout e14ec579b3fb58f4c061ca38c103cc11d58d1673
   ```

2. Apply the pinned APU corrections and copy the Deck-specific source files:
   ```bash
   sed -i 's/\r$//' \
     infones-upstream/{InfoNES.cpp,K6502_rw.h,InfoNES_pAPU.cpp,InfoNES_pAPU.h} \
     infones-upstream/mapper/InfoNES_Mapper_000.cpp
   git -C infones-upstream apply ../patches/infones-apu-register.patch
   git -C infones-upstream apply ../patches/infones-apu.patch
   git -C infones-upstream apply ../patches/infones-apu-quality.patch
   git -C infones-upstream apply ../patches/infones-apu-noise.patch
   cp src/InfoNES_System_Deck.cpp infones-upstream/linux/InfoNES_System_Linux.cpp
   cp src/joypad_input.cpp infones-upstream/linux/
   cp src/nes_audio_mixer.h infones-upstream/linux/
   cp src/nes_apu_noise.h infones-upstream/linux/
   cp src/nes_sram.h infones-upstream/linux/
   ```

3. Build with the cross-compiler:
   ```bash
   cd infones-upstream/linux
   arm-linux-gnueabihf-g++ -static -O2 -fsigned-char -DNDEBUG \
     ../K6502.cpp ../InfoNES.cpp ../InfoNES_Mapper.cpp ../InfoNES_pAPU.cpp \
     InfoNES_System_Linux.cpp joypad_input.cpp -pthread -lm -o InfoNES
   ```

The compiled binary will be `InfoNES`.

The menu itself has no third-party runtime dependencies and can be built with
the same cross compiler:

```bash
arm-linux-gnueabihf-g++ -std=c++11 -Os -Wall -Wextra -Wpedantic -Werror \
  -static src/deck_menu.cpp -lpng -lz -o deck-menu
```

The integrated terminal is built from the vendored GPL-2 source and bundles a
DejaVu Sans Mono runtime font, a static `loadkeys`, and self-contained US ANSI
and Czech QWERTZ console maps:

```bash
nix build .#fbterm-deck -o result-fbterm
nix build .#lua-deck -o result-lua
```

The DECK carousel starts Lua and ECL through the framebuffer terminal. Their
private persistent working directories are `/mnt/data/langs/lua` and
`/mnt/data/langs/lisp`; interpreter binaries remain in
`/mnt/data/nes-deck/langs` and `/mnt/data/nes-deck/ecl`.

## Display Configuration

The Braiins Forge Deck uses a portrait LCD in landscape mode with specific framebuffer requirements. Our patches include:

- 90-degree rotation support
- RGB555 to framebuffer format conversion
- Display offset configuration for proper centering
- TTY-based keyboard input (same as fbDOOM)

These modifications are in `src/InfoNES_System_Deck.cpp` and `src/joypad_input.cpp`.

The Deck framebuffer is 600x1280 RGB565 with a 1280-byte pitch (80 bytes of
padding per row). The port reads `line_length` and `smem_len` from
`FBIOGET_FSCREENINFO`; do not replace the pitch with `xres * bytes_per_pixel`.
Only physical columns 0 through 479 are visible. The menu maps that exact
region to a 1280x480 logical surface. fbterm validates the same geometry and
uses a 16-pixel safe area for the rounded panel, leaving a 1248x448 terminal
viewport. Both reject unexpected geometry or RGB channel offsets instead of
guessing.

Audio uses `/dev/dsp`, backed by ALSA's OSS compatibility plugin. The physical
I2S device is S16_LE stereo; the emulators supply S16_LE mono and the plugin
duplicates it to the hardware channels. InfoNES, Fuse, and CHIP-8 use 44100 Hz.

InfoNES builds each rotated 512x480 image in a cacheable staging buffer and
then publishes contiguous framebuffer rows. Set `INFONES_RUNTIME_DIAGNOSTICS=1`
for 120-frame FPS and render-time windows. `INFONES_VSYNC=1` enables the
driver's blocking fbdev synchronization for experiments; it is disabled by
default because live Mario measurements showed audio/panel phase stalls.
Gambatte produces 32768 Hz, but this Deck's OSS layer falsely echoes that rate
while the live ALSA stream runs at 32000 Hz. The shared runtime therefore
requests the real 32000 Hz rate and explicitly resamples Gambatte audio. The
port requests eight 1024-byte periods. PCM gain is intentionally set in the
mixer because the kernel OSS path bypasses ALSA's userspace softvol.
The touch launcher persists an exact volume from 0 through 100 in
`menu-volume.state` and passes it through both `INFONES_VOLUME_PERCENT` and
`RETRO_DECK_VOLUME_PERCENT`. The header's minus and plus actions move in
5-point steps; 0 is mute. `VOLUME_ON` in `deploy/menu/deck-menu-launcher` and
`deploy/menu/nes-deck.init` is the initial value and the migration value for
the former `on` sound state.

## Project Structure

```
deck-infones/
├── flake.nix              # Nix build configuration
├── src/
│   ├── InfoNES_System_Deck.cpp  # Deck framebuffer/display code
│   ├── libretro_deck.cpp        # FCEUmm, Gambatte, and Fuse host
│   ├── chip8_deck.cpp           # CHIP-8 Deck frontend
│   ├── chip8_core.c             # c-octo adaptation boundary
│   ├── deck_runtime.cpp         # Shared framebuffer/audio/frame clock
│   ├── deck_menu.cpp            # Games, volume, keymap, Wi-Fi, and terminal
│   ├── joypad_input.cpp         # Two THEGamepads plus keyboard fallback
│   ├── nes_audio_mixer.h        # Mixer, DC blocker, and rate conversion
│   ├── nes_apu_noise.h          # Tested 15-bit noise clock helpers
│   └── nes_sram.h               # Tested NES battery-save codec
├── patches/
│   ├── infones-apu-register.patch # Fixes out-of-bounds APU status access
│   ├── infones-apu.patch          # Frame/envelope corrections
│   ├── infones-apu-quality.patch  # Event, noise, triangle, and DPCM fixes
│   └── infones-apu-noise.patch    # Exact noise clocks/length and pulse guard
├── tests/
│   ├── deck_menu_test.cpp      # Menu, Wi-Fi UI, child, state, and geometry
│   ├── deck_wifi_profile_add_test.sh # Atomic profile replacement checks
│   ├── retro_terminal_test.sh   # Scoped layout load and restoration checks
│   ├── joypad_input_test.cpp    # THEGamepad mapping and two-player state
│   ├── chip8_core_test.c        # CHIP-8 pitch and real-ROM core smoke checks
│   ├── nes_audio_test.cpp       # Host-side mixer/resampler checks
│   ├── nes_apu_noise_test.cpp   # LFSR period and high-rate clock checks
│   └── nes_sram_test.cpp        # Battery-save round-trip and damage checks
├── deploy/menu/                 # Catalog compiler, fallback, and S99 service
├── deploy/terminal/             # Terminal launcher and private fontconfig
├── terminal/                    # Vendored GPL-2 fbterm source and provenance
├── nix/ecl-arm-static.nix       # Minimal static ARM ECL 26.5.5 runtime
├── ops/deck-menu/               # Pinned FOSS ROM/license fetcher
├── FOSS_GAMES.md                # Homebrew provenance, licenses, and hashes
├── README.md
└── BUILD.md
```
