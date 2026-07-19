# Retro Deck technical documentation

## Current system

The current deployable system consists of static ARMv7 executables running on
the Deck's OpenWrt userspace. C++ programs implement the dashboard, shared
display and audio runtime, libretro host, CHIP-8 frontend, chiptune player, and
timer. A Go service implements authenticated ROM intake and appearance
configuration. ECL currently provides the interactive Lisp program and runs
the catalog compiler during activation.

The BMC installation presents the dashboard as a scene widget. Games use a
black fullscreen layer surface and a centered gameplay layer surface. The
native runtime can fall back to direct framebuffer output. Audio uses the ALSA
OSS bridge at `/dev/dsp`; emulator timing is independent of the audio writer.

Detailed current build, display, audio, and source contracts are documented in
[`BUILD.md`](../BUILD.md). Third-party identity and license handling are
documented in [`THIRD_PARTY.md`](../THIRD_PARTY.md).

## Target system

Rust becomes the sole first-party native systems language. It owns resource
lifetime, memory and buffer safety, device access, process supervision,
network request boundaries, and foreign interfaces to pinned emulator and
media libraries.

Common Lisp becomes a trusted behavior runtime rather than a device driver.
A supervised ECL worker loads tracked behavior and then root-owned local
overrides. Rust and Lisp exchange bounded, versioned S-expressions. Rust
validates all replies and retains safe built-in behavior when Lisp is absent
or fails.

Audio follows explicit ownership states: closed, priming, active, draining,
and idle. Short cues acquire the device only for their duration. Paused,
muted, hidden, and idle applications release it. Continuous emulator playback
retains it to avoid latency, clicks, and repeated OSS negotiation. Future BMC
widget audio must share one lazy manager rather than opening one permanent
stream per widget.

The complete target layout, migration order, and proof gates are defined in
[`IMPLEMENTATION_PLAN.md`](../IMPLEMENTATION_PLAN.md).
