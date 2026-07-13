# Building InfoNES for Braiins Forge Deck

This guide explains how to build InfoNES from source for the Braiins Forge Deck.

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

3. Build using nix:
   ```bash
   nix build
   ```

   The compiled binary will be at `./result/bin/infones`.

   Build the static ARM touch launcher and the minimal ECL runtime separately:
   ```bash
   nix build .#deck-menu -o result-menu
   nix build -f nix/ecl-arm-static.nix -o result-ecl
   ```

   Those payloads are at `result-menu/bin/deck-menu` and
   `result-ecl/{bin/ecl.bin,lib/ecl/help.doc}`. The ECL expression pins both
   nixpkgs and ECL 26.5.5 and emits a closure-free static ARM runtime.

4. Run the host-side audio checks after mixer changes:
   ```bash
   g++ -std=c++11 -Wall -Wextra -Werror -Isrc \
     tests/nes_audio_test.cpp -o nes_audio_test
   ./nes_audio_test

   g++ -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
     src/deck_menu.cpp -o deck-menu-host
   ./deck-menu-host --geometry-test
   g++ -std=c++11 -O2 -Wall -Wextra -Wpedantic -Werror \
     tests/deck_menu_test.cpp -o deck-menu-test
   ./deck-menu-test
   ```

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
  -static src/deck_menu.cpp -o deck-menu
```

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

Audio uses `/dev/dsp`, backed by ALSA's OSS compatibility plugin. The physical
I2S device is S16_LE stereo; the emulator supplies S16_LE mono at 44100 Hz and
the plugin duplicates it to the hardware channels. The port requests eight
1024-byte periods and retains a signed-16-bit resampling fallback in case
`SNDCTL_DSP_SPEED` is coerced on another image. PCM gain is intentionally set
in the mixer because the kernel OSS path bypasses ALSA's userspace softvol.
The touch launcher passes `INFONES_VOLUME_PERCENT=42` when its sound setting is
on and `0` when it is off. Change `VOLUME_ON` in
`deploy/menu/deck-menu-launcher` and `deploy/menu/nes-deck.init` together to
tune the enabled level from 0 through 100.

## Project Structure

```
deck-infones/
├── flake.nix              # Nix build configuration
├── src/
│   ├── InfoNES_System_Deck.cpp  # Deck framebuffer/display code
│   ├── deck_menu.cpp            # Touch launcher and persistent sound toggle
│   ├── joypad_input.cpp         # Optional TTY keyboard input handler
│   ├── nes_audio_mixer.h        # Mixer, DC blocker, and rate conversion
│   └── nes_apu_noise.h          # Tested 15-bit noise clock helpers
├── patches/
│   ├── infones-apu-register.patch # Fixes out-of-bounds APU status access
│   ├── infones-apu.patch          # Frame/envelope corrections
│   ├── infones-apu-quality.patch  # Event, noise, triangle, and DPCM fixes
│   └── infones-apu-noise.patch    # Exact noise clocks/length and pulse guard
├── tests/
│   ├── deck_menu_test.cpp      # Catalog, sound state, and geometry checks
│   ├── nes_audio_test.cpp       # Host-side mixer/resampler checks
│   └── nes_apu_noise_test.cpp   # LFSR period and high-rate clock checks
├── deploy/menu/                 # Catalog compiler, fallback, and S99 service
├── nix/ecl-arm-static.nix       # Minimal static ARM ECL 26.5.5 runtime
├── ops/deck-menu/               # Pinned FOSS ROM/license fetcher
├── FOSS_GAMES.md                # Homebrew provenance, licenses, and hashes
├── README.md
└── BUILD.md
```
