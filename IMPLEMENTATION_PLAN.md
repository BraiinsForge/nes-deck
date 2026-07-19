# Rust and Common Lisp migration

## Objective

Retro Deck will use Rust for its native appliance runtime and Common Lisp for
trusted, startup-loaded behavior. The first-party Go uploader has been
removed after its Rust replacement passed behavioral, security, and build
parity. First-party C++ will be removed after each replacement passes the same
gates. Third-party emulator implementations remain pinned upstream
dependencies with their provenance and local patches organized in one
predictable hierarchy.

The Rust uploader and current C++ programs form the deployable baseline during
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
9. No migration commit knowingly weakens the existing uploader security,
   save-game persistence, controller ordering, rendering, or recovery path.

## Target source layout

```text
Cargo.toml                    Rust workspace and shared lint policy
Cargo.lock                    pinned Rust dependency graph
crates/
  retro-deck-platform/        display, input, audio, time, and process APIs
  retro-deck-policy/          supervised Lisp worker and S-expression protocol
  retro-deck-dashboard/       dashboard model, renderer, and launcher
  retro-deck-emulator/        libretro and c-octo hosts
  retro-deck-apps/            timer and chiptune applications
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
- Add a tested audio lifecycle state machine before an OSS backend uses it.
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

- Port 10 Seconds first. Rust keeps true monotonic time while Lisp decides
  application policy from validated event data.
- Port CHIP-8 using the pinned c-octo implementation through a narrow foreign
  interface.
- Port chiptunes and implement explicit pause, mute, and idle audio release.

### Phase 4: emulator host and dashboard

- Port the shared display, input, audio, save, and frame-clock runtime.
- Port the libretro host without changing pinned emulator implementations.
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
