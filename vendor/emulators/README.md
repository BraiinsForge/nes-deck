# Emulator vendor tree

Each emulator has one directory containing its source provenance, complete
license record, exact vendored source when applicable, and an ordered local
patch series. Files below `upstream/` are byte-for-byte upstream material and
must not be edited in place.

Local changes belong below `patches/`. List patches in application order in
`patches/series`; keep that file even when the pinned revision needs no local
patches. First-party Rust adapters live in `crates/retro-deck-emulator/`, not
inside an upstream source tree.

GitHub Linguist classifies this hierarchy as vendored so upstream languages do
not obscure the composition of Retro Deck's own code.
