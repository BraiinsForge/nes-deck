# Freely licensed games on Retro Deck

The boot menu retains two freely licensed CHIP-8 games. The other console
libraries contain only owner-supplied ROMs. The pinned fetch script downloads
these CHIP-8 payloads and rejects any checksum mismatch. Generated files remain
ignored by this repository and install under `/mnt/data/roms/chip8/`, with the
source license under `/mnt/data/nes-deck/licenses`.

| Game | Version/source | License | ROM SHA-256 |
| --- | --- | --- | --- |
| Outlaw | [CHIP-8 Archive at `0a41cc2`](https://github.com/JohnEarnest/chip8Archive/tree/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf) | CC0-1.0 | `7e45f3eeeafd3cb825f150b51020df4a49212a556e095387382970636c6be0dc` |
| Space Racer | [CHIP-8 Archive at `0a41cc2`](https://github.com/JohnEarnest/chip8Archive/tree/0a41cc23ad5c9abbb764d041c11ea8c5b77b2bbf) | CC0-1.0 | `409a67b70a0e7d8bde7e38cc4ec5ceb6570b707bded7541f1682c6e7e53c9b90` |

Space Racer is a simultaneous two-player game: controller 1 uses D-pad
up/down for the left ship and controller 2 uses D-pad up/down for the right
ship. Outlaw uses the standard Octo mapping: D-pad moves, A/X fires, and B/Y
maps to Q.

CHIP-8, SCHIP, and XO-CHIP use the c-octo emulator core pinned at
`5f62f185c9e6ae324dcbe9e7fe35ec7c3bdebfb1`, under MIT. The binary installs
its upstream license into the Nix output.

Owner-supplied cartridge images are documented in `roms/README.md`. This
private repository retains them for reproducible setup but makes no claim
about redistribution rights.
