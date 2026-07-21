# Deck platform and runtime notes

This file is the current-state field reference for Retro Deck. It intentionally
does not preserve the chronological deployment diary, old binary hashes,
temporary backup names, or superseded experiments. Git history records those
details when they are needed for archaeology.

## Sources of truth

- `README.md` documents installation and normal use.
- `BUILD.md` documents reproducible builds, tests, and source layout.
- `deploy/menu/README.md` defines dashboard behavior and the catalog contract.
- `ops/deck-wifi/README.md` defines Wi-Fi recovery behavior.
- `ops/deck-wireguard/README.md` defines the userspace WireGuard bundle.
- `ops/bmc/README.md` defines the pinned Wayland canary and rollback workflow.
- `docs/swipe-rendering-postmortem.org` explains the clock swipe incident.
- `terminal/PROVENANCE.md` records the vendored fbterm sources and licenses.

## Hardware and persistent storage

- The target is an ARMv7 Cortex-A7 Braiins Forge Deck running OpenWrt.
- The unit has 256 MiB of RAM, with 128 MiB reserved for CMA on the inspected
  BMC image. A 64 MiB swapfile starts after `/mnt/data` is mounted so a stock
  widget and Retro Deck can coexist without an avoidable global OOM kill.
- `/mnt/data` is the persistent application volume. Deployment refuses to
  activate when that mount is absent.
- Tracked ROMs install under `/mnt/data/roms/<system>/`. The systems are `nes`,
  `gb`, `gbc`, `zx`, and `chip8`.
- Language working directories live under `/mnt/data/langs/`. Chiptunes live
  under `/mnt/data/chiptunes`.
- Runtime state, logs, covers, licenses, and launchers live under
  `/mnt/data/nes-deck`.

## Display and presentation

- The panel framebuffer is 600 by 1280 RGB565 with a reported 1280-byte pitch.
  Each physical row therefore includes 80 padding bytes. Code must use the
  reported stride, not `xres * bytes-per-pixel`.
- The panel is portrait hardware exposed by BMC as a 1280 by 480 logical
  landscape surface. Retro Deck has no separate fbdev presentation path.
- The touchscreen already reports logical landscape coordinates from X
  0 through 1279 and Y 0 through 479. No touch rotation is needed.
- Native content uses a 16-pixel safe inset for the rounded panel corners.
  fbterm uses a 1248 by 448 viewport for the same reason.
- With BMC installed, the dashboard is a full-screen swipeable widget. Games
  use a full-screen black layer-shell surface plus a centered game surface.
  Those temporary surfaces disappear when the game exits, restoring scene
  swiping.
- The active scene is `Visible`; its immediate previous and next scenes are
  `Prepared`, and more distant scenes are `Dormant`. Preparing the immediate
  neighbors supplies a populated buffer before the first swipe and prevents a
  black destination frame. The older generation 14 deployment kept Retro Deck
  dormant while the clock was active and measured about 2.7 MiB RSS. That
  historical measurement is not the standby-memory result for the current
  prepared-neighbor candidate.
- Emulators expand source pixels to the integer-scaled layer buffer before
  submitting it. `patches/bmc-nearest-neighbor-filter.patch` also selects
  nearest-neighbor minification and magnification in Smithay. Apply it with
  `ops/bmc/apply-local-patches.sh` before building the BMC core.
- The menu disables Linux console blanking whenever it starts and unblanks the
  framebuffer after a child returns. The display should never time out while
  Retro Deck is active.

## Dashboard and programs

- `deploy/menu/games.sexp` is the editable catalog. The compiler produces
  `games.tsv` and `palette.tsv`; checked-in copies are tested byte for byte as
  fallbacks.
- Console entries point only below `/mnt/data/roms/<system>/`. Deck-native
  routing identifiers stay below `/mnt/data/nes-deck/games/` and do not need a
  fictional file extension.
- Terminal, Lua, Common Lisp, Python, Scheme, chiptunes, and reboot are native
  built-in entries added by the dashboard. They are not ROMs.
- The dashboard palette uses full RGB semantic roles. A malformed persistent
  override cannot prevent startup; the launcher falls back through the last
  valid generated palette, the checked-in palette, and built-in defaults.
- The dashboard ships only the approved CC0 `gear-knekko-09` settings cog and
  its matching built-in fallback. There is no cog picker.

## Emulators and media

| System | Core | Source geometry | Nominal frame rate |
| --- | --- | --- | --- |
| NES | FCEUmm | 256 by 224 after vertical overscan crop | 60.100 Hz |
| GB and GBC | Gambatte | 160 by 144 | 59.728 Hz |
| ZX Spectrum | Fuse | 320 by 240 core output | 50 Hz |
| CHIP-8 | c-octo based frontend | VM-specific low resolution | 60 Hz host loop |

- NES renders at exact 2x scale as 512 by 448 inside the safe area.
- CHIP-8, SCHIP, and XO-CHIP frames use indexed source pixels and integer
  nearest-neighbor expansion.
- The chiptune player supports GME formats and 44.1 kHz mono or stereo Ogg
  Vorbis. It does not recurse without bounds or follow symbolic links.
- Console emulators display a top-left exit cross. Holding anywhere for two
  seconds terminates the supervised child and returns to the dashboard.

## Controllers and keyboards

- Identical Retro Games THEGamepad controllers are ordered by stable physical
  USB path. Keep them in the same hub ports to preserve Player 1 and Player 2.
- A/X maps to the primary console button. B/Y maps to the secondary button.
  Start maps to Start and Back maps to Select.
- A keyboard fallback maps arrows or WASD to the D-pad, Space to A, Shift to B,
  Enter to Start, and Control to Select.
- ZX Spectrum uses Kempston for Player 1 and Sinclair 2 for Player 2. A/X fires,
  L is Enter, and R is Space. A physical keyboard is passed through to Fuse as
  a Spectrum keyboard instead of being reduced to console controls.
- The dashboard grabs controllers and keyboards only while it is visible. It
  releases them before starting a managed child, then rescans after the child
  exits.

## Audio and saves

- The Deck exposes `/dev/dsp` through its ALSA OSS bridge. The runtime treats
  audio failure as non-fatal so video and input remain usable.
- Menu volume is stored from 0 through 100 in five-point steps and is passed to
  every child. Muting remembers the last audible level.
- BMC owns the ALSA playback lease for native applications. It opens the device
  after the first PCM packet and releases it on an explicit release,
  disconnect, or 250 ms of inactivity. On generation 14, the idle compositor,
  dormant Retro Deck widget, and clock host each hold no `/dev/snd` or
  `/dev/dsp` descriptor.
- FCEUmm reports 48 kHz. The OSS device remains configured at its required
  nominal 48 kHz while the runtime resamples to the measured 47,328-frame
  application clock to avoid slowing emulation.
- Gambatte produces 32,768 Hz and is resampled to the Deck's verified 32 kHz
  OSS rate.
- NES battery RAM is saved atomically beside the ROM as `.srm`. GB and GBC use
  `.sav`, plus `.rtc` when the cartridge exposes a real-time clock. Deployment
  merges ROM directories and preserves these sidecars.
- ZX TAP files are read-only tape media and have no automatic save sidecar.

## Terminal

- The terminal is an integrated static fbterm fork, not an external release
  copied onto the Deck by hand.
- It uses DejaVu Sans Mono, fixed glyph advances, RGB565 channel order, the
  rounded-corner viewport, and the active `/dev/tty1` console.
- The dashboard toggles between US ANSI and Czech QWERTZ keymaps. The launcher
  restores US after fbterm exits.
- The shell mode starts `/bin/ash`. Lua, ECL, MicroPython, and Chibi Scheme
  start in private persistent language directories. ECL uses `rlwrap` and a
  private persistent history file.

## Networking

- Wi-Fi profile writes and Wi-Fi selection are deliberately separate. The
  dashboard writes a validated mode-0600 PSK profile through stdin and never
  scans, reloads, roams, or disconnects the current network.
- The watcher requires association, a `wlan0` IPv4 address, and a default route
  before declaring the connection healthy. It waits through a 90-second boot
  grace and two failed health observations before asking the selector to act.
- Selection is bounded and transactional. It tries saved profiles, restores the
  immediate UCI backup when all candidates fail, and never waits forever after
  rollback.
- The status file contains no credentials. The settings screen shows its state,
  the active SSID, the WLAN address, and the WireGuard address.
- The uploader listens on every IPv4 interface at port 8080. Its password comes
  from the private per-Deck configuration and is never committed.
- WireGuard uses the checked-in ABI-matched TUN module plus userspace
  `wireguard-go`. Each Deck owns a unique private key generated on that Deck and
  a unique operator-configured IPv4 `/32` address. Server identity, endpoint,
  routed prefix, health target, and peer registration live outside the
  repository as private operator configuration.

## Services and deployment

- `/etc/init.d/bmc-compositor` owns the display and spawns the Retro Deck
  widget. Deployment disables a leftover fbdev `nes-deck` service when one is
  present; BMC is the only supported presentation path.
- `/etc/init.d/nes-deck-uploader`, `/etc/init.d/deck-wifi`, and
  `/etc/init.d/deck-wireguard` are independent services.
- `ops/deploy.sh` builds a complete static ARM payload and uploads it to a
  private staging directory. `ops/deploy/activate.sh` validates that tree before
  stopping services, installs it, and attempts to restart stopped services if
  activation fails.
- Application deployment does not edit Wi-Fi. Initial provisioning performs
  guarded Wi-Fi and WireGuard setup before calling the same application
  deployer.

## Current physical acceptance items

The 2026-07-21 generation 14 deployment on `.15` passed host tests, ARM builds,
BMC package activation, Common Lisp policy startup, service checks, and
process-path checks. After 138 seconds at the clock, `bmc-openwrt`,
`retro-deck`, and `bmc-wasm-host` remained alive with zero OOM kills. The
prepared-neighbor candidate is built but has not yet had its one-Deck canary.
Four observations still require eyes and ears on that physical unit:

1. Swipe repeatedly between the clock and Retro Deck, confirm the current
   tabbed dashboard, and check for smearing, flashing, or a black return screen.
2. Compare NES pixel edges after the nearest-neighbor compositor patch.
3. Listen to Kirby and Micro Mages long enough to assess the reported NES audio
   distortion rather than only their title screens.
4. Start a ZX title with the rebuilt valid-memfd frontend and confirm its first
   playable frame.

These are explicit acceptance checks, not claims that unobserved behavior has
already been verified.

## Useful checks

```sh
# BMC and Retro Deck processes
/etc/init.d/bmc-compositor status
pidof bmc-openwrt
pidof retro-deck
pidof bmc-wasm-host
tail -n 100 /var/log/bmc/bmc.log
tail -n 100 /var/log/bmc/widgets.log

# Generated catalog
cmp /mnt/data/nes-deck/menu/games.tsv \
  /mnt/data/nes-deck/state/games.tsv

# Network status without credentials
cat /var/run/deck-wifi/status
ip -4 address show dev wlan0
ip -4 address show dev wg0
ip -4 route show default

# Display and audio facts
cat /sys/class/graphics/fb0/virtual_size
cat /sys/class/graphics/fb0/stride
cat /sys/class/graphics/fb0/bits_per_pixel
cat /proc/asound/card0/pcm0p/sub0/hw_params
```
