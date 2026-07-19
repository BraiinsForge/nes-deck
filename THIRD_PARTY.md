# Third-party software and assets

Retro Deck is built on free software. The dashboard's `(c)` screen reads the
authoritative project, role, and SPDX-style license summary from
[`deploy/menu/credits.tsv`](deploy/menu/credits.tsv).

The deployer installs the corresponding upstream notices and license texts at
`/mnt/data/nes-deck/licenses`. Runtime packages carry their own notices, while
the shared static libraries, the Go runtime used by the checked-in
`wireguard-go` userspace binary, Nixpkgs, and CC0 asset provenance are collected
by `nix/runtime-licenses.nix`. The separately pinned ECL runtime carries ECL,
ASDF/UIOP, Boehm GC, libatomic_ops, GMP, and glibc notices from the exact source
archives used in its build.

Source identity is reproducible from these files:

- `flake.lock` pins the fetched emulator cores and Nixpkgs.
- `vendor/emulators/` records exact vendored sources, revisions, licenses, and
  ordered local patch series.
- `flake.nix` names every native runtime and its linked libraries.
- `nix/ecl-arm-static.nix` pins the independent ECL build environment.
- `FOSS_GAMES.md`, `chiptunes/README.md`, and `deploy/menu/ASSETS.md` record
  the included CC0 games, music, and settings icons.

Owner-supplied ROMs are private data. They are not third-party project
dependencies and are not relicensed by this repository.
