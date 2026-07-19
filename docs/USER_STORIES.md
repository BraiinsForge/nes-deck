# Retro Deck user stories

## Player

- I can navigate the dashboard by touch or either supported controller without
  input lag, repeated commands, or unexplained sound playback.
- I can launch every cataloged system and return to the dashboard through a
  visible, consistent control.
- I can use two controllers as stable Player 1 and Player 2 inputs where the
  game supports simultaneous play.
- I can mute, change volume, and pause applicable programs without another
  process retaining the audio device unnecessarily.
- I can leave and resume games that support persistent saves without losing or
  corrupting their save data.

## Deck owner

- I can upload a supported ROM through an authenticated local web interface,
  and an invalid or duplicate upload cannot replace an existing game.
- I can inspect active Wi-Fi, local IPv4, and WireGuard IPv4 information before
  changing connectivity.
- I can customize dashboard colors without a malformed optional override
  preventing the dashboard from starting.
- I can place trusted Common Lisp overrides in a persistent local directory,
  restart the application, and obtain either the new behavior or a clear
  failure with safe built-in behavior.

## Technician

- I can provision a fresh Deck from a local address with explicit Wi-Fi,
  WireGuard, and uploader credentials, then verify the installed state with a
  read-only health command.
- I can deploy a new release transactionally without deleting ROMs, saves,
  local Lisp overrides, or the previous recoverable installation.
- I can identify which process owns audio and know that idle Retro Deck and BMC
  components release the device.

## Maintainer

- I can reproduce every ARMv7 executable from pinned sources without an
  undeclared Nix runtime closure on the Deck.
- I can update an emulator by changing one documented pin and ordered patch
  series without mixing upstream source into first-party runtime code.
- I can change low-level behavior in idiomatic Rust and application policy in
  Common Lisp, with a narrow versioned boundary between them.
- I can run one documented host check that covers Rust, Lisp, deployment,
  catalogs, rendering, security boundaries, and generated-file drift.
