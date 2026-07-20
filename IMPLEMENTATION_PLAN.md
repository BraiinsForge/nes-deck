# BMC-native Rust and Common Lisp migration

## Objective

Retro Deck is a native Rust application and game launcher presented as a
swipeable scene by the BMC Wayland compositor. "Widget" refers only to that
native compositor surface contract: Retro Deck is not a WASM guest. It is not
a second compositor toolkit, settings service, web framework, or
hardware-management stack. First-party code is mainly idiomatic Rust and
Common Lisp. Emulator sources remain pinned upstream dependencies with ordered
local patches.

The current C++ dashboard and the self-contained Rust candidate are rollback
references while the BMC-backed widget is built. Neither becomes a reason to
preserve duplicate infrastructure. A replacement is selected only after host,
ARMv7, BMC integration, and live Deck checks pass.

## Ownership boundary

| Concern | Owner |
| --- | --- |
| Widget lifecycle, touch, visibility, frame callbacks and DMA-BUF slots | `bmc-widget` |
| GPU canvas, layout, text, icons, images and hit testing | `bmc-render` |
| System brightness, reboot, global sound and Wi-Fi recovery | BMC services and protocols |
| HTTP transport, authentication, sessions, CSRF and multipart parsing | BMC web stack, or an established Rust web stack until integration exists |
| ROM validation, catalog meaning, cover selection and save paths | Retro Deck |
| Game/app state, retro styling and controller mappings | Retro Deck |
| Emulator ABI adaptation and pinned-core patches | Retro Deck |
| Real-time emulator and chiptune PCM | A narrow lazy Rust adapter until BMC exposes streaming audio |
| Trusted local behavior patches | Supervised Common Lisp with a bounded Rust boundary |

BMC sources are dependencies, not copied or vendored into this repository.
When BMC becomes public, pin one reviewed commit. During local integration a
sibling checkout may be selected outside Git, but no machine-specific path or
private network topology is committed.

Retro Deck consumes the public native application boundary that `bmc-main`
provides. When a required compositor, input, rendering, lifecycle, or system
service capability is genuinely missing or defective, fix or extend it in
`bmc-main` as a generally useful facility and consume that facility here. Do
not compensate with a Retro Deck-specific compositor fork or parallel platform
stack.

## Dependency rule

Before adding a first-party subsystem, check in this order:

1. BMC's public widget, render, protocol, platform, audio, and web crates.
2. A maintained Rust crate with a compatible target and license.
3. A small adapter around a pinned upstream implementation.
4. New first-party infrastructure only when the first three cannot satisfy a
   measured Deck requirement.

An exception must name the missing upstream capability and its deletion
condition. Static linking and closure-free deployment are verification
properties, not justification for recreating a mature library.

## Code to retain

- Side-effect-free catalog, palette, launch and application state.
- ROM format checks, safe paths, transactional installation and native saves.
- Controller semantics and emulator-specific keyboard projection.
- Libretro session behavior that has no suitable maintained frontend library.
- The CHIP-8 core adapter and neatly recorded emulator provenance and patches.
- Timer and chiptune product behavior.
- The bounded Common Lisp override contract, after simplifying it to the
  behavior actually required on the device.
- Tests for retained product behavior and safety boundaries.

## Code to replace and delete

- Replace the widget half of `retro-deck-platform` Wayland, shared-memory
  presentation, visibility and touch code with `bmc-widget`.
- Replace dashboard raster primitives, layout, PNG scaling and duplicated
  interaction geometry with `bmc-render`. Retro pixel art remains an asset or
  a small renderer component, not a parallel UI toolkit.
- Send finite dashboard cues through the widget action protocol. Do not keep a
  dashboard OSS worker beside BMC sound ownership.
- Move brightness, reboot and Wi-Fi recovery behind BMC-owned controls. Retro
  Deck must not independently modify those resources.
- Put ROM endpoints behind BMC authentication and Axum routing. Delete the
  hand-written HTTP, session, CSRF, form and multipart implementation once the
  route is integrated.
- Prefer compositor-delivered keyboard input when BMC exposes it. Until then,
  keep only the small evdev keyboard/controller gap, release grabs whenever the
  widget is dormant, and document the upstream capability still missing.
- Split gameplay layer-shell presentation from widget presentation. Keep the
  smallest measured adapter until BMC offers an application/game surface API.
- Remove superseded C++, protocol copies, generated bindings, shell plumbing,
  obsolete tests and unused dependencies as each vertical slice moves.

The pivot must reduce net first-party LOC. No new platform abstraction lands
without deleting the duplicate it replaces. The first review target is below
30,000 Rust code lines including inline tests, excluding pinned upstream
sources. This is a review threshold, not an invitation to game the counter.

## Audio contract

Frantisek Bohacek's guidance is the governing ownership rule: audio is open
only while something needs to play. BMC owns finite widget sounds. Retro Deck
requests a cue and never opens `/dev/dsp` for menu navigation.

Real-time emulator and chiptune PCM is different from BMC's current file-based
`madplay` service. Until BMC supplies a streaming API, one Rust owner per active
game or player opens lazily, writes off the input thread, and closes on mute,
pause, dormancy, exit, idle, or first silence as appropriate. Input never waits
for audio. If a BMC streaming service lands, this adapter is deleted.

## Migration sequence

### 1. Freeze and measure

- Preserve `migration/rust-lisp` as the self-contained reference.
- Perform all integration work on `integration/bmc-widget`.
- Record first-party LOC by crate and identify product, test, adapter and
  duplicate infrastructure before each deletion series.

### 2. Replace the dashboard platform

- Run the dashboard as a native process registered as a swipeable BMC scene;
  do not compile it to WASM or host it inside the WASM runtime.
- Consume `DeckWidgetSurfaceClient`, lifecycle events, touch events, frame
  callbacks and reusable DMA-BUF slots from `bmc-widget`.
- Render the authoritative dashboard screens with `bmc-render` on BMC's GPU
  path, preserving the approved visual design rather than pixel identity with
  the temporary C++ implementation.
- Drive menu cues through widget sound actions.
- Delete the superseded widget protocol, poll, touch, buffer and menu-audio
  implementations immediately after the vertical slice passes.

### 3. Use BMC system and web services

- Remove direct dashboard brightness, reboot and Wi-Fi mutation.
- Agree on a widget-safe system-control surface with BMC instead of binding the
  settings-overlay-only protocol opportunistically.
- Add ROM intake to BMC's authenticated Axum service, keeping only Retro Deck's
  ROM/catalog/storage domain code.
- Keep network deployment unchanged until a separately reviewed BMC-backed
  replacement preserves recovery access and is live-tested.

### 4. Minimize game and application adapters

- Reassess the libretro frontend against maintained crates and RetroArch before
  retaining custom ABI code.
- Keep only measured Deck-specific gameplay presentation, controller, save and
  streaming-audio gaps.
- Share BMC rendering and lifecycle facilities with the timer and chiptune
  player where their surface role permits it.
- Simplify Common Lisp supervision to the startup-loaded patch behavior that is
  actually used, without exposing device descriptors or blocking frame/input
  paths.

### 5. Organize dependencies and remove the fallback

- Keep each emulator under `vendor/emulators/<name>/` with provenance,
  revision, license, ordered `patches/series`, and local patches only.
- Mark upstream sources as vendored for repository language statistics.
- Remove all superseded first-party C++, Go remnants, generated protocol copies
  and dead deployment branches.
- Keep private ROMs, saves, credentials and site Lisp outside executable source
  replacement paths.

## Verification gates

Every retained Rust and Lisp unit passes its focused tests. Before selecting
the BMC widget, run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
sbcl --script lisp/tests/run.lisp
tests/run-host-tests.sh
tests/verify-arm-builds.sh
```

Also build the pinned BMC integration, verify the widget manifest/package,
audit first-party LOC and dependencies, and confirm no copied BMC sources or
machine-specific topology entered Git. The final gate is a controlled live
Deck exercise covering swipe lifecycle, touch, two controllers, keyboard
handoff, menu sound, continuous game audio, launches, saves, ROM upload and
network recovery. Network behavior is never inferred from emulator tests.

## Commit discipline

Each commit is one reviewable replacement, deletion, or product change with an
imperative subject. Run the relevant checks and push it immediately. Preserve
unrelated worktree changes. Do not deploy or mutate a Deck without explicit
authorization.
