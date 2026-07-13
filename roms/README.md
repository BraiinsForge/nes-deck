# Private ROM library

This private repository stores owner-supplied cartridge images in one
canonical tree:

```text
roms/
  nes/
  gb/
  gbc/
  chip8/
```

Filenames are lowercase kebab-case and omit dump-set metadata. The original
payload bytes are preserved; `SHA256SUMS` records their identities after ZIP
extraction and filing.

The repository root is the intake area. A ROM or single-ROM ZIP at the root is
new and has not yet been filed. Validate it, place the extracted image under
the correct system directory, update `SHA256SUMS` and the menu catalog, then
remove the root intake file.

On the Deck, the corresponding runtime tree is `/mnt/data/roms/<system>/`.
Save, RTC, and emulator configuration sidecars live beside their ROM. The
freely licensed games documented in `FOSS_GAMES.md` are reproducibly fetched
into the same layout and are not duplicated here.
