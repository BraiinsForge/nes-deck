# Building InfoNES for Braiins Forge Deck

This guide explains how to build InfoNES from source for the Braiins Forge Deck.

## Prerequisites

The Deck uses an ARM-based processor (armv7), so you'll need a cross-compilation toolchain if you're building from a non-ARM system.

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
   ```

2. Copy the Deck-specific source files:
   ```bash
   cp src/InfoNES_System_Deck.cpp infones-upstream/linux/InfoNES_System_Linux.cpp
   cp src/joypad_input.cpp infones-upstream/linux/
   ```

3. Build with the cross-compiler:
   ```bash
   cd infones-upstream/linux
   make CROSS_COMPILE=arm-linux-gnueabihf- CFLAGS="-static -O2 -fsigned-char" LDFLAGS="-static -lpthread -lm"
   ```

The compiled binary will be `InfoNES`.

## Display Configuration

The Braiins Forge Deck uses a portrait LCD in landscape mode with specific framebuffer requirements. Our patches include:

- 90-degree rotation support
- RGB555 to framebuffer format conversion
- Display offset configuration for proper centering
- TTY-based keyboard input (same as fbDOOM)

These modifications are in `src/InfoNES_System_Deck.cpp` and `src/joypad_input.cpp`.

## Project Structure

```
deck-infones/
├── flake.nix              # Nix build configuration
├── src/
│   ├── InfoNES_System_Deck.cpp  # Deck framebuffer/display code
│   └── joypad_input.cpp         # TTY keyboard input handler
├── README.md
└── BUILD.md
```
