# Rust and Common Lisp migration

## Objective

Retro Deck will use Rust for its native appliance runtime and Common Lisp for
trusted, startup-loaded behavior. The first-party Go uploader has been
removed after its Rust replacement passed behavioral, security, and build
parity. First-party C++ will be removed after each replacement passes the same
gates. Third-party emulator implementations remain pinned upstream
dependencies with their provenance and local patches organized in one
predictable hierarchy.

The Rust runtimes and current C++ dashboard form the deployable baseline during
the remaining migration. A replacement becomes the default only after its own
tests, the complete host suite, the ARMv7 closure audit, and a live Deck check
pass.

## Required properties

1. Rust owns Wayland and framebuffer buffers, input devices, monotonic clocks,
   audio devices, emulator foreign interfaces, process supervision, HTTP
   boundaries, and atomic filesystem updates.
2. Common Lisp owns application policy, declarative UI behavior, catalog
   compilation, and device-local overrides that load after tracked behavior.
3. A malformed, crashing, or unresponsive Lisp override cannot prevent the
   Rust dashboard from starting with safe built-in behavior.
4. Lisp code never receives raw device descriptors or unvalidated pointers.
5. Audio is owned by one Rust manager per active runtime. It opens lazily,
   drains before closing, and releases the device when muted, paused, hidden,
   or idle. Active continuous emulator playback keeps its lease.
6. Input dispatch never waits for audio playback, an audio-device operation,
   or a Lisp reply. Short cues use a bounded nonblocking queue; overload drops
   stale sound feedback instead of delaying touch or controller events.
7. Deployments preserve the root-owned local Lisp override directory and never
   add its contents to Git or the upload web interface.
8. Every upstream emulator has a pinned revision, license record, build
   adapter, and ordered patch series. Generated sources and build outputs are
   not mixed with first-party source.
9. Emulator and frontend compatibility is not a migration requirement. The
   replacement does not need to read retired save encodings, configuration
   formats, command lines, or reproduce accidental implementation behavior.
10. Owner-supplied ROMs and current persistent data remain outside executable
    replacement paths and are never deleted as part of a migration.
11. Network configuration is a separate safety-critical boundary. Emulator
    migration work does not alter live Wi-Fi, WireGuard, routing, or network
    deployment behavior. Any network change requires explicit scope,
    validation, preserved recovery access, and a rollback path.

## Target source layout

```text
Cargo.toml                    Rust workspace and shared lint policy
Cargo.lock                    pinned Rust dependency graph
crates/
  retro-deck-platform/        display, input, audio, time, and process APIs
  retro-deck-policy/          supervised Lisp worker and S-expression protocol
  retro-deck-config/          side-effect-free catalog and console schemas
  retro-deck-dashboard/       dashboard model, renderer, and launcher
  retro-deck-emulator/        libretro and c-octo hosts
  retro-deck-apps/            timer and chiptune applications
  retro-deck-ui/              fixed RGB565 canvas and complete 5x7 font
  retro-deck-uploader/        authenticated ROM and appearance service
lisp/
  retro-deck.asd              tracked Common Lisp system
  package.lisp                single project package
  policy/                     hook registry and worker protocol
  apps/                       tracked application behavior
vendor/
  emulators/
    <project>/
      provenance.md           revision, source, license, and integration notes
      patches/series          ordered local patch list
      patches/*.patch         patches applied in listed order
```

Installed local overrides live outside the tracked source tree at
`/mnt/data/nes-deck/lisp/site.d/*.lisp`. Files load in lexical order after the
tracked Lisp system. Deployment treats this directory as persistent state.

## Runtime boundary

The Rust host starts a disposable ECL worker with pipes connected to standard
input and output. The worker loads tracked Lisp, then the local override
directory, and reports readiness only after all forms load successfully.
Messages are bounded, one-line S-expressions with an explicit protocol
version and request identifier. Rust validates every reply before applying it.

Low-frequency events such as navigation, timer transitions, layout changes,
and cue selection may cross this boundary asynchronously. Raw input is drained
and applied in Rust before any policy request is made. Frame publication,
audio sample production, input draining, emulator callbacks, and filesystem
security do not cross the boundary. If startup, parsing, timeout, or validation
fails, Rust logs the failure, terminates the worker, and uses built-in behavior.

## Migration sequence

### Phase 1: foundation

- Add the pinned Rust workspace with repository-wide formatting, lint, and
  test entry points.
- Add the narrow S-expression protocol and supervised Lisp worker.
- The tested audio lifecycle now drives finite cues, continuous square tones,
  and general bounded PCM workers. PCM callbacks never wait, the OSS stream
  opens lazily, and mute, pause, hide, shutdown, and idle release are explicit.
- Extend ARMv7 verification to reject dynamic binaries and Nix references.

### Phase 2: management service (code complete)

- Ported password configuration, authentication, sessions, CSRF protection,
  origin checks, bounded uploads, ROM validation, atomic catalogs, palette
  updates, BMC scene installation, and embedded assets to Rust.
- Verified password-record compatibility, host tests, and a static,
  closure-free ARMv7 Nix build, then removed the Go module.
- Keep the live Deck login, upload, palette, restart, and password-rotation
  exercise in the final release gate.

### Phase 3: native applications

- The 10 Seconds state machine, renderer, display, nonblocking input, audio,
  clock, process integration, and bounded Common Lisp policy boundary are in
  Rust. Its superseded C++ executable has been removed.
- CHIP-8 uses the checked-in c-octo source through a narrow C adapter owned by
  Rust. Rust owns bounded program loading, configuration, two-controller
  state, crisp fixed-geometry presentation, 60 Hz pacing, and lazy audio. The
  superseded first-party C/C++ host and external source input have been
  removed.
- The chiptune catalog, controls, renderer, Ogg decoder, GME ownership wrapper,
  60 Hz runtime, diagnostics, and atomic volume persistence are in Rust. Audio
  opens lazily off the input path and releases on pause, mute, hide, or exit.
  Its superseded first-party C++ player and shared runtime have been removed.

### Phase 4: emulator host and dashboard

- Shared Rust display, input, frame-clock, and audio adapters are present. The
  PCM path preserves the measured 32,768-to-32,000 Hz and
  48,000-to-47,328 Hz Deck corrections, stream-ring priming, bounded latency,
  and nonblocking callback contract.
- The Rust libretro host owns pinned-core lifecycle, input, adaptive
  nearest-neighbor presentation, lazy audio, exact frame pacing, and bounded
  native save persistence without retired frontend formats.
- NES, GB/GBC, and ZX build as separate static ARMv7 outputs from the same
  Rust host and pinned upstream archives. The superseded C++ host and its
  direct-include tests have been removed.
- Console identity, the bounded catalog, and the complete full-RGB semantic
  palette are shared Rust configuration types. Uploader-specific file access,
  durable override storage, and ROM intake policy remain outside that
  side-effect-free model. The checked-in palette is tested against a compiled
  fallback so a missing or malformed optional palette cannot block startup.
- The Rust dashboard crate now combines base, uploaded, and generated native
  entries behind one duplicate-checked category view with fixed NES, GB, GBC,
  ZX, CHIP-8, and Deck ordering. Its capacity reserves seven native slots
  beyond the shared 64-entry owner and upload catalog limit.
- Standard native applications have unique catalog identities below
  `/mnt/data/nes-deck/games/`. Their executable commands and REPL modes are a
  separate launch contract, so shared launchers no longer masquerade as ROM or
  application-data paths.
- A closed Rust launch classification now maps every console entry, native
  game, REPL mode, chiptune player, terminal, and reboot entry without storing
  executable paths in display data. Unknown native applications fail closed.
- Its pure state machine owns per-category carousel position, modal and
  settings navigation, mute restoration, bounded volume and brightness,
  keymap selection, and typed launch or persistence effects. Input handling
  performs no filesystem, audio-device, process, or network operation.
- Controller routing consumes committed press edges only. D-pad axes keep
  category and carousel movement separate, L/R change volume, and touch
  activation requires matching press and release targets. Audio feedback is
  downstream from these actions and cannot quarantine or delay input. A fixed
  twelve-edge window suspends a flooding controller until one quiet second.
- A device-independent RGB565 canvas now owns clipped drawing, panels, lines,
  text fitting, fixed-capacity labels, and the complete case-sensitive 5x7
  font. The Rust timer uses it without changing any reference pixel snapshot,
  and future Wi-Fi labels can render capitalization unambiguously.
- The Rust catalog screen now renders into one fixed 1280x480 allocation with
  bounded tabs, cards, indicators, cover-art borrowing, approved cog fallback,
  and hit targets that return semantic model actions. Its canonical NES frame
  is pixel-identical to the current C++ renderer, and a host PPM capture uses
  the production Rust pixel path.
- The Rust settings screen is also pixel-identical to its C++ reference. It
  renders case-sensitive SSID, WLAN, WireGuard, selector, login-shell, volume,
  brightness, and keymap values from a bounded read-only view. Its hit targets
  return typed actions only; no network or device mutation occurs in rendering.
- The bounded Rust credits schema rejects malformed, duplicate, empty, or
  excessive manifests. Its intro, perspective crawl, and reduced-motion views
  are pixel-identical to the C++ references while sampling the shared font
  directly instead of retaining per-line raster masks.
- Startup reads the required catalog and optional credits and palette through
  bounded regular-file descriptors that reject final symlinks. Catalog failure
  stops before display setup; credits and palette failures remain observable
  while selecting safe unavailable and compiled-color fallbacks.
- A staged native dashboard binary now drives the production Rust Wayland
  widget, controller discovery and hotplug, touch commits, visibility-aware
  polling, menu/settings switching, and 25 Hz credits frames through one reused
  allocation. It is not packaged or deployed until external effects land.
- Port the dashboard model and renderer, using Lisp only on state changes.
- Generate screenshots from the same Rust renderer used on the Deck.

### Phase 5: dependency organization and removal

- Move emulator provenance and patch series into `vendor/emulators/`.
- Remove superseded first-party C++, headers, and obsolete test harnesses.
- Keep vendored fbterm isolated until a tested Rust terminal replacement is
  available; never describe its code as first-party.

## Verification gates

Each vertical migration commit runs its focused Rust and Lisp tests. Before a
replacement becomes deployable, all of the following must pass:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
sbcl --script lisp/tests/run.lisp
tests/run-host-tests.sh
tests/verify-arm-builds.sh
```

The Lisp command becomes mandatory when the tracked Lisp runtime is added.
The final gate is a read-only health report followed by manual touch,
controller, sound, swipe, upload, and save-game checks on a BMC Deck.

## Commit discipline

Every commit is one independently reviewable structural change, regression
fix, or vertical replacement. Relevant checks run before the commit, and the
commit is pushed immediately. Generated files are committed only when the
runtime consumes them directly and their generator and drift check are also
tracked.
