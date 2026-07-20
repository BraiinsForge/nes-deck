# Retro Deck user documentation

Retro Deck is a touch-first launcher and collection of native frontends for
the Braiins Forge Deck. The authoritative setup and operating instructions are
kept in the following documents:

- [`README.md`](../README.md) covers initial configuration, deployment,
  dashboard operation, controller mappings, uploads, and saves.
- [`DECK_NOTES.md`](../DECK_NOTES.md) records verified hardware behavior,
  connectivity, recovery, and device-specific diagnostics.
- [`deploy/menu/README.md`](../deploy/menu/README.md) describes the installed
  menu, catalog, state, and program layout.
- [`deploy/uploader/README.md`](../deploy/uploader/README.md) describes the
  authenticated ROM and appearance interface and password rotation.

The Rust and Common Lisp migration is in progress. Rust is authoritative for
the ROM uploader, CHIP-8, 10 Seconds, chiptune player, and the shared NES,
GB/GBC, and ZX libretro host. The dashboard remains on its documented C++
implementation until its replacement passes the gates in
[`IMPLEMENTATION_PLAN.md`](../IMPLEMENTATION_PLAN.md). Device-local Lisp
overrides are not yet an available release feature.

Owner-supplied ROMs, save data, credentials, and future local Lisp overrides
are private persistent data. They must not be placed in screenshots, logs,
public reports, or upstream issue attachments.
