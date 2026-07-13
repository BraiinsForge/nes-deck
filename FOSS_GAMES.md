# Freely licensed games on Retro Deck

The boot menu includes eight unmodified homebrew ROMs for NES, Game Boy,
Game Boy Color, and CHIP-8. They come from their authors' repositories or
release pages. `ops/deck-menu/fetch-foss-games.sh` downloads the pinned builds
and rejects any checksum mismatch. ROM binaries remain ignored by this
repository; the Deck keeps them under
`/mnt/data/nes-deck/roms`, with license texts under
`/mnt/data/nes-deck/licenses`.

| System | Game | Version/source | License | ROM SHA-256 |
| --- | --- | --- | --- | --- |
| NES | Falling | [xram64/falling-nes at `52dcb8a`](https://github.com/xram64/falling-nes/tree/52dcb8a951200562e696dfc2aba5d4d14edd0078) | MIT | `e22b947542c2d7e595bf84725b333be7af8189c5965b9c53e356a249c7d79943` |
| NES | Thwaite | [v0.04 release](https://github.com/pinobatch/thwaite-nes/releases/tag/v0.04) | GPL-3.0-or-later | `a2df24d9c9f72e56c2fdc4c703becc47a5700ad0158da8208247635ebeb3779c` |
| NES | Concentration Room | [v0.02a release](https://github.com/pinobatch/croom-nes/releases/tag/v0.02a) | GPL-3.0-or-later, with an exception for exact published ROM copies | `2ce17df1ad66a8a0533c0a8739f5b5ebe275c264924bbe350c42c5ac0394f20e` |
| NES | robotfindskitten | [v0.10 release](https://github.com/pinobatch/rfk-nes/releases/tag/v0.10) | zlib | `13abbea91f553780c88c2a85a40b7e86fd5916026c01bfc4f88a8b9b9a9abfe1` |
| GB | Adjustris | [v1.1 release](https://github.com/tbsp/Adjustris/releases/tag/v1.1) | CC0-1.0 | `b6c8affe6d906419cfc99ff459718f33a1868af03254a2f65cea2a9430394712` |
| GBC | Geometrix | [source at `8f5467e`](https://github.com/AntonioND/geometrix/tree/8f5467ec225e21d67b2e6621eabede70dc6cc8fa) | GPL-3.0-or-later | `56efdf82118e5faf22511c18dd1fc2ab8bc0c5e44cd634b8e06050ff08124586` |
| CHIP-8 | Outlaw | [CHIP-8 Archive at `0a41cc2`](https://github.com/JohnEarnest/chip8Archive/tree/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf) | CC0-1.0 | `7e45f3eeeafd3cb825f150b51020df4a49212a556e095387382970636c6be0dc` |
| CHIP-8 | Space Racer | [CHIP-8 Archive at `0a41cc2`](https://github.com/JohnEarnest/chip8Archive/tree/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf) | CC0-1.0 | `409a67b70a0e7d8bde7e38cc4ec5ceb6570b707bded7541f1682c6e7e53c9b90` |

All four NES games use mapper 0, the simplest NES cartridge layout and a
supported InfoNES path. Thwaite intentionally flickers some explosion and
smoke effects; its upstream warning notes that flicker can be unsafe for
people with photosensitive seizures.

Adjustris is a monochrome Game Boy ROM. Geometrix advertises Game Boy Color
support while retaining compatibility with monochrome hardware. Space Racer
is a simultaneous two-player game: controller 1 uses D-pad up/down for the
left ship and controller 2 uses D-pad up/down for the right ship. Outlaw uses
the standard Octo mapping: D-pad moves, A/X fires, and B/Y maps to Q.

## Emulator provenance

GB and GBC use the Gambatte libretro core pinned at
`dfc165599f3f1068c40a0b7ad6fe5f161283d483` and licensed GPL-2.0-only. The
Deck build uses the core's RGB565 output and Cortex-A7/NEON tuning. CHIP-8,
SCHIP, and XO-CHIP use the c-octo emulator core pinned at
`5f62f185c9e6ae324dcbe9e7fe35ec7c3bdebfb1`, under MIT. Both binaries install
their upstream license into their Nix output.

The existing Mario files are user-supplied cartridge dumps/downloads and are
not FOSS. This private repository retains them for the owner's reproducible
setup, but does not claim a license or redistribution rights for them.
