# Retro Deck technical documentation

## Current system

The current deployable system consists of static ARMv7 executables running on
the Deck's OpenWrt userspace. Rust implements the CHIP-8 host, 10 Seconds game,
authenticated ROM intake, the shared libretro host, Common Lisp policy
boundary, and shared Wayland, input, timing, and lazy OSS audio layers. C++
remains in the dashboard, its legacy shared runtime, and the chiptune player
while those pieces are migrated. ECL provides the interactive Lisp program,
runs the catalog compiler during activation, and loads bounded device-local
behavior policy.

The Rust audio foundation includes fixed-capacity PCM buffering, direct
stereo-to-mono downmixing, callback-stable linear resampling, worker-side gain,
and a lazy OSS stream worker. NES, GB/GBC, and ZX now use this worker through
one Rust libretro host linked separately to each pinned upstream core. Audio
callbacks only attempt a bounded queue operation; opening, priming, writing,
and closing `/dev/dsp` stay on the worker thread.

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

Input and audio are independent execution paths. Touch and controller events
update Rust state immediately and only try to enqueue a small cue identifier
on a bounded channel. They never open `/dev/dsp`, write samples, wait for a
sound child, or observe an audio cooldown. The audio worker may coalesce or
drop stale cues when it falls behind. Continuous PCM producers follow the
same contract: they only attempt a queue lock and bounded wakeup. Contention
or overload discards old sound rather than delaying an emulator callback.
Opening, ring priming, gain, resampling, writing, draining, resetting, and
retry timing all remain on the dedicated audio thread. The same rule applies
to Common Lisp: policy work is supervised and deadline-bound, never an
input-loop dependency.

The complete target layout, migration order, and proof gates are defined in
[`IMPLEMENTATION_PLAN.md`](../IMPLEMENTATION_PLAN.md).
