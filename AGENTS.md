# Repository Instructions

## ROM intake

- `roms/<system>/` is the canonical location for owner-supplied ROMs in this
  private repository. Supported system directories are `nes`, `gb`, `gbc`,
  `zx`, and `chip8`.
- A ROM or single-ROM ZIP placed at the repository root is unprocessed intake.
  Never commit it at the root. Validate the payload, extract it when needed,
  give it a lowercase kebab-case filename, and file it under the matching
  system directory.
- Update `roms/SHA256SUMS`, `deploy/menu/games.sexp`, and the checked-in
  `deploy/menu/games.tsv` whenever a filed ROM is added to the menu.
- Console catalog paths must use `/mnt/data/roms/<system>/<filename>`. Deck
  applications are not ROMs and remain under `/mnt/data/nes-deck/games/`.
- Freely licensed ROMs fetched by `ops/deck-menu/fetch-foss-games.sh` are
  reproducible build output and are not duplicated in the tracked ROM library.
- Preserve `.sav`, `.rtc`, `.state`, and emulator configuration sidecars when
  moving a Deck ROM. Keep each sidecar beside its ROM in the same system
  directory.
