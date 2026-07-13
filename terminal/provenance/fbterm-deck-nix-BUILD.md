# Building Framebuffer Terminals for Braiins Forge Deck

## Prerequisites

The Deck uses an ARM-based processor (armv7), so you'll need a cross-compilation toolchain if you're building from a non-ARM system.

## Option 1: Using Nix (Recommended)

Nix is a reproducible build system and package manager that ensures consistent builds across different machines. The easiest way to build is using the provided Nix flake, which sets up the complete cross-compilation environment automatically.

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
# Build fbterm (default)
nix build github:BraiinsForge/deck-fbterm
```

The binaries will be at:
- `./result/bin/fbterm`
- `./result/bin/yaft`

**Build Variants:**
- `#default` or `#fbterm` - Full-featured fbterm with BGR color fix, fontconfig, and rotation support

### Development Workflow

For active development and iteration:

1. Clone this repository with submodules:
   ```bash
   git clone --recursive git@github.com:BraiinsForge/deck-fbterm.git
   cd deck-fbterm
   ```

2. Build using the local flake:
   ```bash
   nix build .#fbterm
   ```

3. The compiled binaries will be in `./result/bin/`.

## Option 2: Manual Toolchain Setup

If you prefer not to use Nix, you can manually install an ARM cross-compiler and build the dependencies.

### Install ARM Toolchain

**Debian/Ubuntu:**
```bash
sudo apt install gcc-arm-linux-gnueabihf g++-arm-linux-gnueabihf
```

**Arch Linux:**
```bash
yay -S arm-linux-gnueabihf-gcc
```

### Dependencies

fbterm requires:
- fontconfig
- freetype2
- gpm (optional, for mouse support)

These must be cross-compiled for armv7 or obtained as static libraries.

### Clone Repository

```bash
git clone --recursive git@github.com:BraiinsForge/deck-fbterm.git
cd deck-fbterm
```

### Build fbterm

```bash
cd fbterm-fork
./configure --host=arm-linux-gnueabihf
make CXXFLAGS="-static" LDFLAGS="-static"
```

**Note:** Static linking requires static versions of all dependencies (fontconfig, freetype2, etc.), which can be complex to set up. The Nix build handles this automatically.

## Display Configuration

The Braiins Forge Deck uses a portrait LCD in landscape mode with BGR color format. Our fbterm fork includes:

- BGR565 framebuffer color fix
- Software screen rotation (`-r` option)
- Font baseline adjustment for improved glyph positioning

These modifications are in the `fbterm-fork/` submodule.

## Project Structure

```
deck-fbterm/
├── flake.nix              # Nix build configuration
├── fbterm-fork/           # Patched fbterm source (submodule)
├── Minecraftia-Regular.ttf # Recommended font
├── README.md              # User guide
└── BUILD.md               # This file
```

## Troubleshooting

### Build fails with missing dependencies

The Nix build automatically handles all dependencies. If building manually, ensure you have static libraries for fontconfig, freetype2, and their transitive dependencies.

### Font rendering issues

If fonts render incorrectly, try:
1. Using the Minecraftia font (renders best)
2. Adjusting font baseline with `-B` option
3. Forcing font dimensions with `-W` and `-H` options
