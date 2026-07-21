# Build and test Retro Deck

Retro Deck uses Nix for reproducible ARMv7 hard-float builds. The generated
executables are static and run on the Deck's OpenWrt userspace without copying
a Nix store closure to the device.

For ordinary installation, create the private setup configuration and use the
complete deployment command in [README.md](README.md):

```sh
./ops/configure-deck.sh
./ops/deploy.sh
```

This document covers individual builds, verification, tests, and platform
details for development.

## Prerequisites

Install Nix with flakes enabled. Host tests also require Rust 1.94 or newer
with Cargo, rustfmt, and Clippy; a C++ compiler; `pkg-config`; libpng; and
ImageMagick.

All first-party Rust uses the same 1.94 language level as BMC. The flake follows
BMC's nixpkgs input, so native widget and ARM runtime builds use the same Rust
package set instead of maintaining a second compiler boundary.

On Debian or Ubuntu:

```sh
sudo apt-get install \
  build-essential imagemagick libpng-dev pkg-config
```

Then clone the private repository:

```sh
git clone git@github.com:BraiinsForge/retrodeck.git
cd retrodeck
```

The first Nix build downloads the pinned cross toolchain and may take several
minutes. Later builds reuse the Nix store.

Each native runtime receives an explicit local source set. Editing the menu,
for example, invalidates `deck-menu` without rebuilding unrelated emulators.
Keep new source and header files in the corresponding source set near the top
of `flake.nix`; do not add the complete `src/` directory as a build input.

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
nix build --no-link --print-out-paths .#rlwrap-deck
nix build --no-link --print-out-paths .#lua-deck
nix build --no-link --print-out-paths .#python-deck
nix build --no-link --print-out-paths .#chibi-deck
nix build --no-link --print-out-paths .#chiptune-deck
nix build --no-link --print-out-paths .#rom-uploader
nix build --no-link --print-out-paths .#rom-uploader-host
nix build --no-link --print-out-paths .#runtime-licenses
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
| `rlwrap-deck` | `bin/rlwrap` |
| `lua-deck` | `bin/lua` |
| `python-deck` | `bin/python` |
| `chibi-deck` | `bin/chibi-scheme` plus Scheme modules |
| `chiptune-deck` | `bin/chiptune-deck` |
| `rom-uploader` | `bin/rom-uploader` |
| `rom-uploader-host` | Native `bin/rom-uploader` configuration helper |
| `runtime-licenses` | Shared runtime and asset notices |
| ECL expression | `bin/ecl.bin`, runtime library, and notices |

Check that a package has no Nix runtime references before deploying it:

```sh
out=$(nix build --no-link --print-out-paths .#chiptune-deck | tail -n 1)
file "$out/bin/chiptune-deck"
test -z "$(nix-store -q --references "$out")"
```

The expected executable is a statically linked 32-bit ARM EABI5 binary.

Build and inspect the complete deployable matrix with:

```sh
tests/verify-arm-builds.sh
```

This rejects a missing executable, a non-ARM or dynamically linked binary, a
Nix store reference, an incomplete ECL or fbterm runtime, and a changed CC0
music payload.

## Run the host test suite

The test runner compiles into a temporary directory and leaves the worktree
clean:

```sh
tests/run-host-tests.sh
```

It covers libretro lifecycle and persistence, controller and keyboard input,
PCM queuing and resampling, dashboard geometry and behavior, ROM catalog,
cover cache, Wi-Fi profile helper, rlwrap-backed terminal lifecycle, Rust
display/audio runtime, timer and chiptune behavior, and the CHIP-8 core.

The suite runs the strict Rust formatter, lints, and tests for both the root
runtime workspace and the isolated BMC-native dashboard workspace. Uploader
coverage includes authentication, Axum request limits and multipart extraction,
ROM validation, atomic storage, process control, and the Paper UI contract.

Run shell checks on deployment code with:

```sh
nix shell nixpkgs#shellcheck -c shellcheck -x \
  ops/lib/deck-config.sh ops/check-deck.sh ops/configure-deck.sh ops/deploy.sh \
  ops/deploy/activate.sh ops/provision-deck.sh \
  deploy/menu/nes-deck-swap.init \
  tests/run-host-tests.sh tests/deploy_config_test.sh \
  tests/deploy_activation_test.sh tests/check_deck_test.sh \
  tests/nes_deck_swap_test.sh \
  tests/verify-arm-builds.sh
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
scp -r root@DECK-IP:/mnt/data/nes-deck/covers /tmp/deck-covers
ops/deck-menu/render-screenshots.sh deploy/menu/games.tsv \
  /tmp/deck-covers "$HOME/retro-deck-screens"
```

The output contains every game selection, settings variants, animated and
reduced-motion FOSS credits, the Wi-Fi keyboard, reboot confirmation, timer,
and a contact sheet.

## Platform details

The Deck CPU is ARMv7 Cortex-A7 hard-float. Its panel is a portrait 600x1280
RGB565 framebuffer used as a 1280x480 logical landscape display. The physical
pitch is 1280 bytes, including 80 bytes of padding per row, and only physical
columns 0 through 479 are visible. Code must use the stride reported by
`FBIOGET_FSCREENINFO`, not `xres * bytes_per_pixel`.

The menu fills the complete 1280x480 logical surface. Emulators and the
chiptune player use the shared scaler with a 16-pixel safe inset for the
rounded display. fbterm uses a 1248x448 viewport for the same reason. The Rust
emulator host validates every frame and adapts its nearest-neighbor source map
when a core changes geometry without reallocating compositor buffers.

On BMC compositor installations, Retro Deck is a fullscreen scene widget.
The menu submits event-driven XRGB8888 shared-memory buffers through the Deck
widget protocol, so the compositor can move it during scene swipes. A launched
game maps a fullscreen black layer surface plus a centered game layer surface.
The emulator keeps its native frame clock and submits frames independently of
the widget callback limit. The client expands gameplay frames to their
integer-scaled layer size with nearest-neighbor sampling, then the compositor
maps the resulting buffer 1:1. BMC's Smithay renderer defaults to linear
minification and magnification, which can still soften pixel boundaries during
the rotated composition pass. Apply the tracked local patch before building a
BMC image for Retro Deck:

```sh
ops/bmc/apply-local-patches.sh /path/to/bmc-main
nix build --no-link /path/to/bmc-main#deck-packages.core.pkg
```

The patch selects nearest-neighbor filtering for both directions. The script
is idempotent and refuses a source tree whose patch context does not match.
When the game exits, both layer surfaces disappear and scene swiping resumes.

The legacy `ops/deploy.sh` route installs its rollback widget under
`/mnt/data/bmc-widgets/retro-deck`.
If `bmc-compositor` is present, deployment stops it, adds one idempotent Retro
Deck scene to `/etc/bmc_config.json`, disables the legacy fbdev menu service,
enables a 64 MiB swapfile before BMC starts, and restarts the compositor. The
swap is needed because 128 MiB of the Deck's 256 MiB RAM is reserved for CMA;
without it, a stock BMC widget plus Retro Deck can trigger global OOM while
the first fullscreen SHM frame is faulted in. Existing swapfiles are left
untouched if they cannot be enabled. The original configuration is retained
once as `/etc/bmc_config.json.retro-deck.bak` before the first scene edit.

BMC owns ALSA and one central audio-device lease. Every managed foreground
application receives an inherited bounded datagram channel for signed 16-bit
PCM. Retro Deck's libretro, chiptune, CHIP-8, and timer paths perform only
nonblocking submissions; stereo sources are downmixed before transport, and a
full channel drops samples instead of delaying input or emulation.

BMC opens mono ALSA playback lazily on the first packet, enables ALSA rate
conversion for the producer's declared rate, and applies the packet's volume.
It drains and closes after 250 ms without samples, or discards immediately on
an explicit release, disconnect, or application exit. Retro Deck explicitly
releases on mute, pause, hide, stop, and shutdown. Dashboard navigation sounds
use BMC widget actions; finite application cues are rendered once at startup
and use the same bounded PCM channel. No selected Retro Deck Rust path opens
`/dev/dsp` or owns a playback thread.

The Rust libretro host keeps three persistent Wayland SHM frame slots and
drops a new presentation when all slots remain compositor-owned. It never
waits for a buffer release in the input, emulation, or audio callback path.

## Source layout

```text
retrodeck/
├── crates/                      first-party Rust workspace
│   ├── retro-deck-apps/         native app models, renderers, and runtimes
│   ├── retro-deck-audio/        validated PCM, volume, and tone types
│   ├── retro-deck-config/       typed catalog, palette, and system contracts
│   ├── retro-deck-dashboard/    isolated native BMC widget workspace
│   ├── retro-deck-emulator/     libretro host and c-octo boundary
│   ├── retro-deck-platform/     gameplay Wayland, input, and BMC PCM client
│   ├── retro-deck-policy/       bounded Lisp protocol and supervisor
│   ├── retro-deck-ui/           fixed retro raster primitives for applications
│   └── retro-deck-uploader/     authenticated ROM and appearance service
├── chiptunes/                  CC0 seed tracks and provenance
├── deploy/
│   ├── menu/                   catalog, launcher, and procd service
│   ├── terminal/               fbterm wrapper, fontconfig, and keymaps
│   ├── uploader/               uploader service and credential plumbing
│   └── widget/                 BMC manifest, launcher, and scene installer
├── nix/                        ECL and runtime-specific Nix expressions
├── ops/
│   ├── bmc/                    external BMC patch application
│   ├── deck-menu/              covers, screenshots, and FOSS CHIP-8 fetcher
│   ├── deck-wifi/              profile-only Wi-Fi helper
│   ├── deploy/                 validated on-Deck activation transaction
│   ├── lib/                    shared strict deployment configuration parser
│   ├── check-deck.sh           read-only installed health report
│   └── deploy.sh               local build, staging, and transfer
├── patches/                    pinned upstream fixes
├── protocol/                   gameplay layer-shell and rollback protocols
├── roms/                       private canonical ROM library and checksums
├── lisp/                       tracked Common Lisp policy runtime
├── src/                        C++ dashboard rollback pending the live gate
├── terminal/                   vendored fbterm source and provenance
├── tests/                      host regression suite
├── vendor/emulators/           pinned emulator source and provenance
├── flake.nix                   pinned cross-build definitions
└── README.md                   deployment and operation guide
```

The exact on-device file contract and strict catalog schema are documented in
[deploy/menu/README.md](deploy/menu/README.md).
The conservative first-party rewrite baseline is recorded in
[docs/CODE_SIZE.md](docs/CODE_SIZE.md).
