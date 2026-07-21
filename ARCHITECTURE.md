# Retro Deck architecture

## Product boundary

Retro Deck is a native Rust application and game launcher presented as a
swipeable scene by the BMC Wayland compositor. "Widget" means the native BMC
surface lifecycle contract. Retro Deck is not a WASM guest, compositor fork,
device-management stack, or private UI toolkit.

First-party product code is Rust and Common Lisp, plus one small C adapter for
c-octo. Emulator implementations are pinned upstream dependencies with local
provenance, complete hashes, and ordered patch series.

## Ownership

| Concern | Owner |
| --- | --- |
| Scene lifecycle, touch, visibility, frame callbacks, and DMA-BUF slots | BMC `bmc-widget` |
| GPU canvas, layout, text, icons, images, and hit testing | BMC `bmc-render` |
| Keyboard and controller discovery, focus, hotplug, and routing | BMC input services |
| Declared application launch, supervision, and dashboard return | BMC application service |
| Native package closure and generation activation | BMC package tooling |
| System settings, reboot, global sound, and physical audio device | BMC services |
| ROM validation, catalog meaning, covers, saves, and game mappings | Retro Deck |
| Emulator ABI adaptation and recorded upstream patches | Retro Deck |
| Timer, chiptune, CHIP-8, and emulator PCM production | Retro Deck applications |
| Trusted local behavior patches | Supervised Common Lisp policy |
| Network recovery and provisioning | Explicit operations scripts |

BMC sources are pinned dependencies, not copied into this repository. When a
required compositor, rendering, input, application, or audio capability is
missing or defective, implement the general facility in `bmc-main` and consume
it here. Do not build a parallel platform inside Retro Deck.

## Dependency rule

Before adding first-party infrastructure, check in this order:

1. BMC's public widget, render, protocol, platform, audio, and web crates.
2. A maintained crate or library with a compatible target and license.
3. A small adapter around a pinned upstream implementation.
4. New first-party infrastructure only for a measured Deck-specific gap.

Any exception must identify the missing upstream capability and the condition
under which the local implementation can be deleted. Static linking and a
closure-free payload are verification requirements, not reasons to recreate a
mature library.

## Dashboard and applications

The dashboard lives in the isolated `crates/retro-deck-dashboard` workspace.
It consumes BMC native APIs directly, renders the approved tab and carousel UI
through `bmc-render`, and sends logical launch requests to BMC. Catalog data
never supplies executable paths.

Foreground games and programs are declared BMC applications. The smallest
remaining presentation adapter creates gameplay layer-shell surfaces because
BMC does not yet expose a dedicated game-surface API. It owns no dashboard
layout, device discovery, or compositor lifecycle.

The standalone Axum uploader remains until BMC exposes a suitable authenticated
extension route. It uses established Axum, Tokio, Serde, and cryptographic
libraries. Retro Deck owns only ROM validation, transactional storage, palette
data, and catalog refresh behavior around that web boundary.

## Audio contract

Frantisek Bohacek's guidance is the ownership rule: the physical audio device
is open only while something needs to play. Retro Deck never opens ALSA or
`/dev/dsp`.

Dashboard cues use BMC widget actions. Each BMC-managed foreground application
receives a bounded inherited PCM channel. Input, emulation, and decoding paths
submit nonblocking packets; transport pressure drops samples instead of
delaying touch, controller input, or a frame.

BMC opens ALSA lazily after the first packet and releases it after 250 ms of
inactivity. Explicit release also happens on mute, pause, hide, shutdown,
application exit, and channel disconnect. This is a shared BMC facility, not a
Retro Deck-specific playback service.

## Common Lisp policy

Common Lisp is a trusted startup-loaded behavior layer. The worker loads the
tracked policy tree and then root-owned files from
`/mnt/data/nes-deck/lisp/site.d` in lexical order. Rust and Lisp exchange one
bounded, versioned S-expression per line.

The wire vocabulary, nesting, value count, string, integer, and deadline limits
are enforced on both sides. Missing, late, malformed, or rejected policy falls
back to deterministic Rust behavior. Lisp never receives device descriptors
and never runs in an input, rendering, emulation, or audio callback.

## Emulator dependencies

Each recorded emulator lives under `vendor/emulators/<name>/` with:

- an upstream project and immutable revision
- the exact retained source files
- the upstream license
- `patches/series` in application order
- hashes covering every retained file and patch

`tests/vendor_emulators_test.sh` rejects symlinks, missing provenance, an
incomplete hash manifest, or disagreement between the patch directory and
series file. `.gitattributes` marks emulator and fbterm sources as vendored so
repository language statistics represent first-party code.

## Network boundary

Deployment never edits, reloads, or disconnects Wi-Fi. Provisioning and
recovery are separate, explicit operations with private per-device
configuration outside Git. No machine-specific addresses, credentials, peer
registration commands, or local checkout paths belong in this repository.

Network changes require their own review and live recovery test. Emulator,
dashboard, or audio tests cannot establish network safety.

## Verification

The principal local gates are:

```sh
tests/run-host-tests.sh
tests/verify-arm-builds.sh
nix build --no-link --print-out-paths .#retro-deck-widget
sbcl --script lisp/tests/run.lisp
```

The live gate covers repeated scene swipes, touch, two controllers, keyboard
handoff, menu cues, continuous game audio, application return, saves, ROM
upload, and network recovery. Current first-party size and measurement rules
are in [`docs/CODE_SIZE.md`](docs/CODE_SIZE.md).
