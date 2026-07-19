# c-octo provenance

- Project: c-octo
- Upstream: https://github.com/JohnEarnest/c-octo
- Revision: `5f62f185c9e6ae324dcbe9e7fe35ec7c3bdebfb1`
- License: MIT, preserved in `LICENSE.txt`
- Vendored file: `upstream/octo_emulator.h`
- SHA-256: `5431d6cdb47d0853036f5490cefaf672f2b17f2bbf8f8fb5c68d677dffc078fd`

The vendored header is byte-for-byte identical to `src/octo_emulator.h` at the
revision above. Retro Deck compiles it behind the narrow native adapter owned
by `retro-deck-emulator`; application and platform code never depend on its C
structure layout.

Apply entries from `patches/series` in order before compilation. The current
revision needs no local source patches.
