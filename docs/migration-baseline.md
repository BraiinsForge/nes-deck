# RetroDeck migration baseline

This document records the implementation that the Rust and Common Lisp
migration must replace. The running product remains authoritative whenever this
summary and observed behavior disagree.

## Contract

- Preserve the native Wayland product one-to-one from the user's perspective.
- Preserve layout, colors, labels, borders, fonts, animation, timing, sounds,
  touch, keyboard, controller input, launch behavior, saves, and return flows.
- Keep external emulators and established dependencies. Replace only
  first-party C, C++, and Go code.
- Load editable Common Lisp from the device at startup and make Lisp the
  orchestrator and policy owner.
- Keep Rust limited to native mechanisms such as Wayland, buffers, input,
  non-blocking audio, process control, and narrow operating-system interfaces.
- Prefer maintained libraries and small adapters over reimplementation.
- Avoid speculative services, frameworks, hardening, and test volume.
- Reduce total Rust and Common Lisp source below the current measured baseline.

## Physical source-line budget

The baseline uses physical lines, including comments and blank lines. It
excludes shell, protocol XML, generated files, assets, and vendored third-party
code.

| Area | Lines |
| --- | ---: |
| `src/` C and C++ implementations and headers | 13,174 |
| `ops/deck-menu/` C++ screenshot tools | 305 |
| Production Go uploader | 2,016 |
| Existing Common Lisp catalog compiler | 414 |
| **Production and tool baseline** | **15,909** |
| First-party C and C++ tests | 2,144 |
| Go tests | 531 |
| **Baseline including tests** | **18,584** |

Target fewer than 15,909 production Rust and Common Lisp lines and fewer than
18,584 total lines including their focused tests. Do not meet the target by
compressing formatting, generating source, or moving first-party behavior into
another language.

Do not count `terminal/fbterm/` toward the replacement surface. It is vendored
third-party software and remains an external dependency. Apply the same rule to
libretro cores, c-octo, Game Music Emu, Ogg/Vorbis, Wayland libraries, ECL, and
other mature dependencies.

## Current component boundaries

### Dashboard

`src/deck_menu.cpp` is the 4,499-line dashboard and supervisor. It owns startup,
menu state, rendering composition, touch targeting, keyboard and controller
navigation, settings, Wi-Fi entry, application launch, child return handling,
and the primary polling loop.

The dashboard is partially split into:

- `menu_catalog.cpp`: catalogs and built-in Deck applications
- `menu_credits.cpp`: animated and reduced-motion credits
- `menu_state.cpp`: persistent volume, brightness, and keymap state
- `menu_sound.cpp`: synthesized menu cues and their worker process
- `menu_ui.cpp`: bitmap text and pixel primitives
- `menu_network.cpp`, `menu_io.cpp`, and `menu_text.cpp`: narrow helpers
- `deck_wayland.cpp`: widget, layer-shell, shared-memory, and Wayland input

### Shared native runtime

`deck_runtime.cpp`, `deck_wayland.cpp`, and `joypad_input.cpp` provide the
shared framebuffer, Wayland, input, audio, scaling, and frame-clock mechanisms.
They are the clearest initial boundary for thin Rust primitives.

The current Wayland client uses generated bindings for the Deck widget protocol
and wlr-layer-shell. It submits XRGB8888 shared-memory buffers. The fbdev
fallback writes RGB565 frames with the device-reported stride and rotates the
1280x480 logical canvas into the 600x1280 physical framebuffer.

### Applications and emulators

- `libretro_deck.cpp` hosts the external FCEUmm, Gambatte, and Fuse cores.
- `chip8_deck.cpp` and `chip8_core.c` adapt the external c-octo core.
- `chiptune_deck.cpp` uses external Game Music Emu and Ogg/Vorbis libraries.
- `ten_seconds_deck.cpp` implements the native 10 Seconds application.

Preserve the external cores and libraries. Replace only the Deck-owned hosts,
adapters, and application policy.

### Uploader

The uploader contains 2,016 production Go lines and 531 test lines. It provides
an owner-facing HTTP login, ROM upload, palette editing, catalog persistence,
and BMC scene installation. It uses only the Go standard library, but combines
HTTP serving, authentication, validation, storage, UI generation, and setup
operations in one binary.

Preserve the visible upload and palette workflows. Do not automatically recreate
its current service structure or defensive complexity in Rust.

### Existing Common Lisp

`deploy/menu/compile-catalog.lisp` is a 414-line ECL program that validates
`games.sexp` and palette overrides and emits TSV consumed by C++. It is useful
validation code, but it is not yet the product orchestrator.

The deployment already ships static ECL 26.5.5 for ARMv7. Its current build has
threads, DFFI, and the compiler disabled. The migration must prove the smallest
reliable Rust/ECL boundary before committing to an integration shape.

## Static user-visible contract

- Use a 1280x480 logical surface.
- Preserve nearest-neighbor pixels and the custom 5x7 bitmap font.
- Preserve 4-pixel cut corners and 4-pixel default borders.
- Preserve system order: NES, GAME BOY, GBC, ZX SPECTRUM, CHIP-8, DECK.
- Show at most three centered 216x264 cards with the established spacing,
  arrows, indicators, covers, fallback art, and title truncation.
- Require press and release over the same touch target for activation.
- Preserve every settings, Wi-Fi, credits, reboot, and child-exit touch region.
- Preserve the full-screen two-second touch hold that exits an unmanaged child.
- Preserve the four-second double-confirmation window for reboot.
- Preserve exact menu cue notes and durations:
  - volume: 660 Hz for 60 ms, then 880 Hz for 60 ms
  - previous: 523 Hz for 35 ms
  - next: 659 Hz for 35 ms
  - confirm: 659 Hz for 25 ms, then 880 Hz for 30 ms
  - back: 659 Hz for 25 ms, then 440 Hz for 30 ms
- Preserve state and ROM paths below `/mnt/data` and preserve emulator arguments,
  environment, save formats, and sidecars.

Colors, labels, app definitions, ordering, timing values, the 10 Seconds clock
skew, sound sequencing, interaction rules, and other editable policy belong in
startup-loaded Lisp. Pixel buffers, input descriptors, audio output, Wayland
objects, and process primitives belong in Rust.

## Dashboard audio defect

Menu audio was already asynchronous: `MenuSoundPlayer::play` forks a child that
performs the blocking `/dev/dsp` write. Touch appeared blocked because
`menu_sound_blocks_input` deliberately discarded every touch report while that
child was alive and for its 60 ms tail.

Commit `47c2b36` corrects the reference implementation so touch and keyboard
remain responsive while a cue plays. Controller quarantine remains unchanged.
The migration must preserve the cue waveform and trigger timing while keeping
audio work and waits out of the Wayland/input event path.

## Native host checkpoint

Native ABI 3 now provides the first dashboard-side Wayland mechanism without
replacing the working dashboard. Rust binds `wl_compositor`, `wl_shm`,
`wl_seat`, and `deck_widget_manager_v1`; creates and configures the widget
surface; manages three XRGB8888 shared-memory buffers with release backpressure;
and queues the existing clamped down, motion, up, and cancel touch reports.
Common Lisp decides whether to open, dispatch, present, consume touch, close, or
honor shutdown. Default startup deliberately leaves the display closed.

Host tests cover frame geometry, touch clamping, Lisp policy conversions, the
ECL boundary, generated protocol bindings, and static ARM linkage. A deployed
ARM smoke also confirms ABI 3 and both one-argument Wayland callbacks while the
display is closed. The allocated Deck cannot advertise the custom protocol
because its firmware has no BMC compositor, so actual wire events, scene
lifecycle, compositor hit testing, and physical Wayland touch remain acceptance
work rather than inferred parity.

At this checkpoint the physical Rust and Common Lisp footprint is 1,868
production lines, including the existing catalog compiler, and 2,149 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Dashboard policy checkpoint

Startup-loaded `lisp/policy.lisp` now owns the exact system order and labels,
all 22 semantic dashboard colors, executable routes, built-in Deck applications
and their append order, launch arguments and environment ordering, volume and
brightness steps, controller limits, reboot text, terminal label, reduced-motion
environment name, and dashboard timing values. Its launch-plan functions retain
the GBC-to-GB route, Deck and chiptune argument differences, touch supervision,
terminal console mirroring, and reboot handling from the C++ dashboard.

`startup.lisp` loads this editable policy before an optional device-local
`local.lisp`. Deployment installs both tracked Lisp files, preserves
`local.lisp`, and validates the ARM/ECL startup as part of activation. The C++
dashboard remains authoritative and continues to render and launch applications
until the Lisp-orchestrated replacement reaches physical parity.

At this checkpoint the physical Rust and Common Lisp footprint is 2,111
production lines, including the existing catalog compiler, and 2,589 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Native fbdev checkpoint

Native ABI 4 adds a direct-fbdev presentation mechanism without changing the
Wayland implementation or replacing the C++ dashboard. Rust uses the narrow
Linux framebuffer ioctl and mmap interface, validates the device-reported
600x1280 RGB565 color fields and stride, builds the rotated 1280x480 image in a
staging buffer, and publishes only completed active rows. Common Lisp controls
open, close, logical-size queries, and 24-bit solid-frame presentation. Default
startup leaves fbdev closed.

Compile-time layout checks cover both 64-bit hosts and the 32-bit ARM ABI. Pure
mechanism tests cover geometry rejection, RGB565 conversion, rotation, stride
padding, and corner placement. The ARM/ECL smoke covers ABI 4 and closed-display
callbacks. On the development Deck, a supervised smoke stopped the dashboard,
presented accent `#xfe6c27` as RGB565 `#xfb64`, captured the 1,638,400-byte
stride-aware scanout, verified all 614,400 active pixels with zero mismatches,
and restored a healthy C++ dashboard. The solid frame validates physical open,
ioctl, mmap, stride, conversion, and publication; physical orientation of a
nonuniform Lisp-rendered frame remains for the next rendering slice.

At this checkpoint the physical Rust and Common Lisp footprint is 2,492
production lines, including the existing catalog compiler, and 3,084 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Native canvas checkpoint

Native ABI 5 adds one startup-owned 1280x480 RGBA canvas backed by pinned
`tiny-skia` 0.12 with only its `std` feature. Rust exposes solid clear and clipped,
non-antialiased integer rectangle fills, then presents the same completed canvas
through either the existing triple-buffered Wayland path or the rotated fbdev
path. Common Lisp owns colors and composition calls. Its wrappers reject values
that cannot cross the fixnum, signed-coordinate, or unsigned-dimension boundary.
Default startup still leaves both displays closed.

Focused tests cover opaque channel order, exact clipping, RGBA-to-XRGB8888 and
RGB565 conversion, rotation, stride padding, ECL callback arity, Lisp wrappers,
and static ARM linkage. On the development Deck, a supervised Lisp smoke cleared
to policy `:background` and drew 320x120 policy-colored rectangles in all four
logical corners: `:accent`, `:selected`, `:wifi-focus`, and `:title`. The smoke
captured all 1,638,400 stride-aware framebuffer bytes and verified every one of
the 614,400 logical pixels through `physical-row = 1279 - logical-x` and
`physical-column = logical-y`, with zero mismatches. It then restored a healthy
C++ dashboard. Physical Wayland presentation remains blocked by the Deck
firmware's missing compositor.

At this checkpoint the physical Rust and Common Lisp footprint is 2,709
production lines, including the existing catalog compiler, and 3,407 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Validation baseline

Established on 2026-07-22:

- `./tests/run-host-tests.sh`: passed
- Static ARM build and complete deployment: passed
- Development Deck health check: passed
- Dashboard framebuffer capture: visually matches the current 1280x480 menu
- Development Deck: `root@10.0.0.17`, ARMv7, BOS 2025-11-18 nightly
- `/dev/mmcblk0p4`: ext4 and persistently mounted at `/mnt/data`

The allocated Deck firmware does not contain `bmc-compositor`, so the deployed
reference currently uses the supported direct-fbdev fallback. Use it for touch,
audio, launch, emulator, and framebuffer comparisons. Install a compatible BMC
image before claiming Wayland parity.

Still require physical acceptance for:

- touch responsiveness while every menu cue plays
- controller and keyboard behavior
- exact borders, colors, animation, and transition timing
- every external emulator, save path, and return flow
- Wayland widget movement and game layer surfaces
- chiptune and timer behavior
- uploader and palette editing

## Migration discipline

1. Preserve the working implementation until a replacement slice passes its
   host checks and physical comparison.
2. Migrate narrow vertical slices rather than creating a parallel framework.
3. Record exact behavior before moving policy into Lisp.
4. Reuse existing assets, cores, libraries, paths, and launch contracts.
5. Delete superseded C, C++, and Go only after demonstrated parity.
6. Keep tests focused on migration boundaries and user-visible regressions.
7. Recount Rust and Common Lisp after every substantial slice.
