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

## Bitmap UI checkpoint

Native ABI 6 adds the exact reference 5x7 bitmap font as one clipped, scaled
glyph operation. Its 95 printable ASCII mappings, punctuation, lowercase forms,
and unknown-byte fallback match `menu_ui.cpp` row for row. Lisp remains
responsible for UTF-8-to-ASCII display fallback, glyph advance, text width,
scale selection, ellipsis fitting, centering, straight borders, 4-pixel cut
rectangles, and panel composition. One native call per glyph avoids the original
per-pixel ECL crossing cost while leaving labels and layout directly editable in
`lisp/ui.lisp`.

A complete font hash protects every byte mapping. A deterministic 1280x480
fixture exercises policy colors, a cut-corner panel, a straight outline,
centered text, fitted ellipsis, and non-ASCII `?` fallback. The C++ reference and
Rust canvas independently produce RGB565 FNV-1a hash `414079453e1344d5`, while
Lisp tests protect the exact primitive sequence and coordinates. Deployment now
installs and health-checks `startup.lisp`, `ui.lisp`, and `policy.lisp` while
continuing to preserve optional `local.lisp`.

On the development Deck, the same high-level Lisp fixture rendered through ABI
6 and fbdev, yielding a 1,638,400-byte stride-aware capture that matched the C++
reference hash after all 614,400 logical pixels were unrotated. The supervised
smoke then restored a healthy C++ dashboard. Physical Wayland presentation
remains blocked by the missing compositor firmware.

At this checkpoint the physical Rust and Common Lisp footprint is 3,017
production lines, including the existing catalog compiler, and 3,897 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Static dashboard checkpoint

Startup now loads editable `dashboard.lisp` after the native UI and policy
layers and before optional owner `local.lisp` overrides. Lisp owns the exact
credits and settings controls, populated system tabs, three-card carousel
window, selection geometry, pixel-cut cards, cartridge fallback art, compact
built-in Deck logos, mirrored arrows, indicators, and status footer. Geometry,
labels, colors, and composition order remain ordinary device-editable Lisp.

A deterministic nine-game fixture fixes the six populated tabs, shifted NES
carousel, selected third game, long-title fitting, four indicators, and status
text. The authoritative C++ renderer pins the complete logical RGB565 frame at
FNV-1a `65b48f5f3b66d535`; focused Lisp tests pin matching layout metadata,
selected fills and footer text, plus empty-system, single-card, and covered-game
fallback behavior. Covered catalog entries deliberately keep deterministic
fallback art until the narrow raster-blit slice arrives, while the working C++
dashboard remains deployed and authoritative.

On the development Deck, the installed Lisp fixture rendered through ABI 6 and
fbdev. Its 1,638,400-byte stride-aware capture produced the same
`65b48f5f3b66d535` hash after all 614,400 logical pixels were unrotated with
`physical-row = 1279 - logical-x` and `physical-column = logical-y`. The
supervised fixture then restored a healthy C++ dashboard. Physical Wayland
presentation remains blocked by the missing compositor firmware.

At this checkpoint the physical Rust and Common Lisp footprint is 3,335
production lines, including the existing catalog compiler, and 4,282 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Dashboard raster checkpoint

Native ABI 7 adds three small process-local raster operations: load a cover,
load a dimension-checked PNG, and draw a cached handle. Rust uses the maintained
`png` 0.18 decoder and retains the authoritative P6 PPM fallback rather than
introducing an image framework. Common Lisp owns the editable cover directory
and settings-icon path, preloads assets before interaction, caches handles, and
chooses their exact dashboard placement. Clearing the Lisp cache also releases
the corresponding native raster storage.

Cover intake preserves PNG priority, PPM fallback only when PNG is absent,
regular-file byte bounds, 1..2048 PNG dimensions, aspect-preserving nearest
reduction to 600x378, alpha flattening against each game's policy color, and
xterm-256 quantization. Drawing preserves the centered square crop and nearest
sampling into the 200x200 card art region. The approved 23x23 settings PNG keeps
its original alpha and the reference RGB565 blend while scaling to the centered
50x50 control image. Missing or invalid images retain the established fallback
art.

A deterministic full-frame fixture uses the approved gear PNG both as the
settings asset and as the selected cover. The C++ reference pins its logical
RGB565 FNV-1a hash at `5c932dc59681241e`. On the development Deck, the installed
ABI 7 Lisp/native fixture produced a 1,638,400-byte stride-aware capture with
the same hash after all 614,400 logical pixels were unrotated. The supervised
fixture then restored a healthy C++ dashboard. Physical Wayland presentation
remains blocked by the missing compositor firmware.

At this checkpoint the physical Rust and Common Lisp footprint is 4,087
production lines, including the existing catalog compiler, and 5,251 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Dashboard touch boundary checkpoint

Native ABI 8 adds one synchronous `evdev` 0.13.2-backed Goodix reader for the
fbdev fallback. Rust scans only numbered `/dev/input/event*` devices, requires
the authoritative device-name substring, exact 0..1279 and 0..479 absolute
ranges, and `BTN_TOUCH`, then uses a non-blocking descriptor and best-effort
exclusive grab. The small report state machine preserves coordinate clamping,
press and release edges, every `SYN_REPORT`, and the reference
`SYN_DROPPED` resynchronization boundary. Wayland and evdev now return the same
five-value touch report to Lisp.

Common Lisp owns primary-dashboard targeting and state transitions. It preserves
target priority, half-open bounds, press and release over the same target,
drag-off and cancellation behavior, tab selection and reset, carousel wrapping,
status clearing, and the absence of a cue when the active tab is tapped. Accepted
navigation renders and presents first, then triggers the existing asynchronous
cue without consulting controller-only audio quarantine. Settings, credits,
card launch, keyboard, and controller actions remain outside this narrow slice.

A shared deterministic trace covers two Next activations, a drag from Next to
Previous, a GAME BOY tab activation, and a repeated active-tab tap. The C++
reference pins the corresponding logical RGB565 hashes at
`9f7ec7647982e7bd`, `de67cf4c35ff2b4d`, and `4e9094bcf7a7f9e5`; Lisp pins the
same state, render, and cue sequence. Focused tests also process a second touch
while native audio reports active, preserving the known-defect fix.

Host tests, static ARM/ECL verification, `nix flake check`, and complete
activation passed. With the C++ dashboard stopped under an automatic restore
trap, the deployed native process opened and exclusively owned the physical
Goodix device. The installed Lisp fixture then rendered its initial NES frame
through fbdev; its 1,638,400-byte capture unrotated to C++ hash
`0ef078d4dc7a53bd`. The normal C++ dashboard was restored healthy after each
exercise. No physical touch occurred during the unattended 60-second report
window, so actual panel navigation and cue-overlap responsiveness still require
an operator at the Deck.

At this checkpoint the physical Rust and Common Lisp footprint is 4,586
production lines, including the existing catalog compiler, and 6,034 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Dashboard credits checkpoint

Startup-loaded `credits.lisp` now owns the exact credits TSV contract, labels,
archive path, wrapping, crawl construction, source positions, starfield,
perspective geometry, speed and cycle timing, reduced-motion columns,
unavailable state, close control, and same-target close touch transition. The
working C++ dashboard remains authoritative and deployed; this slice adds no
new production service or replacement menu loop.

Native ABI 9 added bounded descriptor-based regular-file reads plus cached
bitmap text masks and exact projected-text drawing on the existing canvas. Rust
preserves the C++ perspective equations, floor/ceiling sampling, clipping, fade,
and RGB565 alpha-256 blend while Lisp supplies all content, geometry, colors,
and timing. ABI 10 adds a four-word RGB565 canvas hash for parity fixtures and
transports elapsed time as 16 ASCII hexadecimal digits, preserving signed
64-bit timing beyond the ARM ECL fixnum range without moving timing policy into
Rust.

The authoritative C++ renderer pins animated full frames at 0, 2,000, 20,000,
and 600,000,000 ms to `94ebf079be6e596b`, `1f14f6b786549363`,
`6267b51f6f787c83`, and `f62d9d0147c7461a`. Reduced-motion frames at 0 and
60,000 ms both pin to `9a44bcef4a13dde3`. Host tests, `nix flake check`, static
ARM verification, and complete deployment passed. On the development Deck, all
six installed Lisp/native frames produced 1,638,400-byte stride-aware captures
with those same hashes after all 614,400 logical RGB565 pixels were unrotated by
`physical-row = 1279 - logical-x` and `physical-column = logical-y`. Animated
and reduced-motion captures were also inspected visually, then the normal C++
dashboard was restored healthy. Physical Wayland presentation remains blocked
by the missing compositor firmware.

At this checkpoint the physical Rust and Common Lisp footprint is 5,551
production lines, including the existing catalog compiler, and 7,306 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Dashboard settings checkpoint

Startup-loaded `settings.lisp` now owns the exact settings labels, geometry,
state paths, selection order, rendering, and action sequencing. It reproduces
the network summary, volume, brightness, terminal, keymap, close control, and
status footer, including the reduced footer scale for long messages. All eight
half-open touch targets require press and release over the same target.
Controller previous/next selection wraps in the original order, confirm
activates the selection, and back produces the close plan.

Lisp also owns volume decrement, mute, last-audible restore, brightness
clamping, US/CZ keymap toggling, persistence plans, completion state, and menu
cue effects. Volume stop or confirmation audio is emitted only after its state
write succeeds; failed volume writes emit no audio, and a failed confirmation
tone preserves the successful value while showing the original failure status.
Brightness and keymap cues remain unconditional, matching the C++ sequence.
Wi-Fi editing and terminal launch remain later integration slices. The working
C++ dashboard is still authoritative and deployed, and native ABI 10 is
unchanged.

The authoritative C++ renderer and ARM/ECL smoke test pin these frames:

| Fixture | RGB565 FNV-1a hash |
| --- | --- |
| volume 42, brightness 60, US, volume-down selected | `46d1527abb9f2bcb` |
| muted volume, brightness 60, US, volume-up selected | `c2c55ee7eb47608b` |
| volume 42, maximum brightness, US, brightness-up selected | `6e348df7ca27725f` |
| volume 42, brightness 60, CZ, keymap selected | `99ed5871b55b5f6b` |
| volume 42, brightness 60, US, Wi-Fi selected | `65f7d573c69bccbb` |
| volume 42, brightness 60, US, terminal selected | `9cabcc3df5188ce3` |
| confirmation-tone failure status | `05a5652bb03e0b8b` |
| reduced-scale long status | `773a6a165672bd8b` |

Host policy and C++ fixture tests, `nix flake check`, static ARM verification,
complete deployment, and the Deck health check passed. With the C++ dashboard
stopped under an automatic restore trap, each installed Lisp/native fixture
validated its in-memory canvas hash, presented through fbdev, and produced a
1,638,400-byte stride-aware capture. Unrotating all 614,400 logical RGB565
pixels with `physical-row = 1279 - logical-x` and `physical-column = logical-y`
reproduced all eight hashes. The selected controls, mute, maximum brightness,
CZ keymap, Wi-Fi, terminal, tone-failure status, and reduced-scale status
captures were inspected visually. The normal C++ dashboard was then restored
healthy. Physical Wayland presentation remains blocked by the missing
compositor firmware.

At this checkpoint the physical Rust and Common Lisp footprint is 6,037
production lines, including the existing catalog compiler, and 8,013 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Dashboard Wi-Fi editor checkpoint

Startup-loaded `wifi.lisp` now owns the exact editor labels, geometry, key rows,
field limits, helper path, rendering, hit testing, editing state, validation,
and save/back action plans. It preserves the 30-key alphabet layout, uppercase
mode, 42-key symbol layout, active field borders, masked password tail, status
and network footers, and every half-open control and key bound. Touch requires
press and release over the same target. Menu controller and keyboard commands
remain modal: only Back closes the editor.

SSID and password edits retain the original 32- and 63-character limits and
clear stale status after every accepted target, including no-op delete and
full-field insertion. Save validation accepts only printable ASCII, requires an
SSID of 1 through 32 characters and a password of 8 through 63 characters, and
prepares the existing helper with exactly `ssid`, newline, password, newline on
standard input. A successful completion clears the in-memory password and shows
the deferred-use status; failure retains both fields and displays the helper
error. Save always produces the Confirm cue, editing produces Next, and Back
produces the original dashboard status and Back cue. Existing C++ remains
responsible for helper execution until the full Lisp loop is integrated. The
working C++ dashboard remains authoritative and deployed, native ABI 10 is
unchanged, and terminal launch remains a later slice.

The authoritative C++ renderer and ARM/ECL smoke test pin these frames:

| Fixture | RGB565 FNV-1a hash |
| --- | --- |
| empty lowercase alphabet editor | `d6be2f43c4faf0e6` |
| empty uppercase alphabet editor | `7682dc83b0062730` |
| uppercase password field with `NETWORK` and masked `password` | `a5c18f4c41654088` |
| symbol keyboard with the same populated fields | `f919741f85fe2c31` |

Host tests, `nix flake check`, static ARM verification, complete deployment, and
the Deck health check passed. With the C++ dashboard stopped under an automatic
restore trap, each installed Lisp/native fixture validated its in-memory canvas
hash, presented through fbdev, and produced a 1,638,400-byte stride-aware
capture. Unrotating all 614,400 logical RGB565 pixels with
`physical-row = 1279 - logical-x` and `physical-column = logical-y` reproduced
all four hashes. Lowercase, uppercase, populated password, and symbol captures
were inspected visually, then the normal C++ dashboard was restored healthy.
Physical Wayland presentation remains blocked by the missing compositor
firmware.

At this checkpoint the physical Rust and Common Lisp footprint is 6,462
production lines, including the existing catalog compiler, and 8,641 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets without
compressed or generated first-party source.

## Settings terminal process checkpoint

Startup-loaded `process.lisp` now owns terminal titles, the exact starting and
return statuses, native-result validation, launch-plan validation, and the
required menu-audio finish before process handoff. It consumes the existing
Lisp launch policy unchanged: executable, one mode argument, the selected
`RETRO_DECK_KEYMAP`, label, touch supervision, and console mirroring remain
editable without rebuilding Rust. The exact shell results are preserved:
`TERMINAL ERROR - CHECK LOG`, `TERMINAL DID NOT START`, `RETURNED FROM
TERMINAL`, `TERMINAL EXITED`, nonzero status, signal, and generic stopped
variants.

Native ABI 11 adds only the four-argument `RUN-TERMINAL` mechanism and a fixed
five-field result. The Rust supervisor closes direct fbdev while retaining an
open Wayland widget, snapshots `/dev/tty0`, starts a separate child process
group with default TERM/INT/HUP/PIPE handling, and distinguishes exec failure.
It polls in 40 ms slices, retries fbdev touch discovery at most once per second,
requires an uninterrupted full-screen two-second hold, sends process-group
SIGTERM, escalates to SIGKILL after four seconds, and restores keyboard mode,
termios, cursor, wake, and blanking state. On Wayland it samples `/dev/fb0`
every 100 ms, applies the authoritative RGB565 scanout rotation, and presents
through the existing triple-SHM widget buffers.

The C++ dashboard remains authoritative and deployed. `RETRODECK:MAIN` still
does not enter the replacement dashboard loop, so this callable Lisp slice
cannot alter the working menu before the remaining input and return-loop slices
reach parity.

Host tests and `nix flake check` passed. The static ARM/ECL smoke exercised the
real callback under QEMU with exact mode/keymap propagation, clean exit,
nonzero exit, signal exit, and exec failure. ABI 11 and all editable Lisp files
were installed on the ARMv7 Deck. An installed harmless fixture received
exactly `shell` and `cz`, returned `TERMINAL EXITED`, and left the C++ dashboard
healthy. A second physical fixture ignored SIGTERM in both parent and
grandchild; terminating the native host caused process-group SIGTERM and the
four-second SIGKILL escalation, returned signal 9, removed the group, and again
left the dashboard healthy. The Deck health check passed. Current fbdev-only
firmware cannot physically exercise Wayland console mirroring, and an operator
is still required for an actual two-second Goodix touch-return acceptance.

At this checkpoint the physical Rust and Common Lisp footprint is 7,137
production lines, including the existing catalog compiler, and 9,552 lines with
focused Rust and Lisp tests. This remains below the 15,909/18,584 budgets
without compressed or generated first-party source.

## Validation baseline

Updated on 2026-07-23:

- `./tests/run-host-tests.sh`: passed
- `./tests/verify-arm-builds.sh`: passed
- `nix flake check`: passed
- Static ARM build and complete deployment: passed
- Development Deck health check: passed
- Static Lisp dashboard framebuffer hash matched the complete C++ reference
- Raster fixture hash matched C++ at `5c932dc59681241e` on the Deck
- ABI 8 opened the physical Goodix device and matched navigation fixture hash
  `0ef078d4dc7a53bd` through fbdev
- ABI 10 matched all six animated and reduced-motion credits frame hashes
  through the installed ARM/ECL and physical fbdev paths
- ABI 10 matched all eight dashboard settings frame hashes through the
  installed ARM/ECL and physical fbdev paths
- ABI 10 matched all four dashboard Wi-Fi editor frame hashes through the
  installed ARM/ECL and physical fbdev paths
- ABI 11 launched exact terminal fixtures through ARM/ECL, classified clean,
  nonzero, signal, and exec-failure results, and physically verified process-
  group TERM/KILL supervision on the Deck
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
- the terminal's physical two-second Goodix touch-return hold
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
