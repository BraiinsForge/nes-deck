# Retro Deck technical documentation

## Selected architecture

Retro Deck's dashboard is a native ARMv7 process registered as a swipeable BMC
scene. It consumes `bmc-widget` for surface lifecycle and touch,
`bmc-render` for GPU-backed UI, BMC's focused gamepad and keyboard routing,
and the BMC application service for declared child launches. System settings,
reboot, Wi-Fi, and the physical audio device remain BMC responsibilities.

The main Rust workspace owns Retro Deck product behavior: typed catalogs and
palette data, ROM validation and native saves, the CHIP-8 adapter, 10 Seconds,
the chiptune player, the authenticated Axum ROM uploader, and one shared
libretro host for the pinned NES, GB/GBC, and ZX cores. Games currently use a
small gameplay-only layer-shell presentation adapter; it is not a second
dashboard toolkit or compositor.

Common Lisp is a trusted, startup-loaded behavior layer. A supervised worker
loads tracked policy and then a root-owned device-local override. Rust and Lisp
exchange bounded, versioned S-expressions; invalid, late, or missing replies
fall back to safe Rust behavior. The resident worker is reused across policy
requests and replaced only after a real failure.

## Audio and input contract

Dashboard navigation requests BMC-owned finite sounds through the widget
action protocol. BMC-managed games and programs receive an inherited bounded
PCM channel. Retro Deck performs nonblocking packet submission only: it never
opens ALSA or `/dev/dsp`, creates a private playback thread, or waits for sound
from an input or emulation callback. Transport pressure drops audio rather than
delaying touch, a controller event, or a frame.

BMC owns one central audio-device lease. Its application receiver opens ALSA
after the first PCM packet, applies gain, drains after brief inactivity, and
closes immediately on explicit release, disconnect, or application exit.
Retro Deck sends that release when playback is muted, paused, hidden, stopped,
or shut down. Emulator timing therefore remains independent of device I/O.

BMC owns device discovery, hotplug, focus, and routing. Retro Deck receives
semantic controller reports and keyboard events, then applies only its
product-specific mappings. Common Lisp policy is supervised and deadline
bound, never part of the input or audio execution path.

The checked-in C++ dashboard and legacy deployment route are rollback material
until the controlled live-Deck gate selects the BMC packages. They are not the
selected production architecture and carry no compatibility obligation after
that gate.

Detailed build and source contracts are in [`BUILD.md`](../BUILD.md), third
party provenance is in [`THIRD_PARTY.md`](../THIRD_PARTY.md), and the remaining
selection gates are in [`IMPLEMENTATION_PLAN.md`](../IMPLEMENTATION_PLAN.md).
