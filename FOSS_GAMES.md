# FOSS NES games on the Deck

The boot menu includes four unmodified homebrew ROMs obtained from their
authors' repositories or release pages. `ops/deck-menu/fetch-foss-games.sh`
downloads the pinned builds and rejects any checksum mismatch. ROM binaries
remain ignored by this repository; the Deck keeps them under
`/mnt/data/nes-deck/roms`, with license texts under
`/mnt/data/nes-deck/licenses`.

| Game | Version/source | License | Mapper | ROM SHA-256 |
| --- | --- | --- | --- | --- |
| Falling | [xram64/falling-nes at `52dcb8a`](https://github.com/xram64/falling-nes/tree/52dcb8a951200562e696dfc2aba5d4d14edd0078) | MIT | 0 (NROM) | `e22b947542c2d7e595bf84725b333be7af8189c5965b9c53e356a249c7d79943` |
| Thwaite | [v0.04 release](https://github.com/pinobatch/thwaite-nes/releases/tag/v0.04) | GPL-3.0-or-later | 0 (NROM) | `a2df24d9c9f72e56c2fdc4c703becc47a5700ad0158da8208247635ebeb3779c` |
| Concentration Room | [v0.02a release](https://github.com/pinobatch/croom-nes/releases/tag/v0.02a) | GPL-3.0-or-later, with an exception for exact published ROM copies | 0 (NROM) | `2ce17df1ad66a8a0533c0a8739f5b5ebe275c264924bbe350c42c5ac0394f20e` |
| robotfindskitten | [v0.10 release](https://github.com/pinobatch/rfk-nes/releases/tag/v0.10) | zlib | 0 (NROM) | `13abbea91f553780c88c2a85a40b7e86fd5916026c01bfc4f88a8b9b9a9abfe1` |

All four use mapper 0, the simplest NES cartridge layout and a supported
InfoNES path. Thwaite intentionally flickers some explosion and smoke effects;
its upstream warning notes that flicker can be unsafe for people with
photosensitive seizures.

The existing `mario.nes` is a user-supplied cartridge dump/download and is not
FOSS. The menu can list it locally, but this repository does not distribute it
or claim any license for it.
