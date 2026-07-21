# Dashboard data bundle

This directory contains the checked-in catalog, palette, cover helper, and
small system hooks used by the native Retro Deck dashboard. The dashboard
binary itself is built from `crates/retro-deck-dashboard` and installed through
BMC's native widget and application package format.

No launcher or framebuffer dashboard service lives here. BMC owns the scene
lifecycle and starts `retro-deck` from its generation-managed profile.

## Installed files

`ops/deploy.sh` installs these data files below
`/mnt/data/nes-deck/menu/`:

| File | Purpose |
| --- | --- |
| `games.sexp` | Reviewed source catalog |
| `games.tsv` | Validated runtime catalog |
| `palette.tsv` | Validated default dashboard colors |
| `compile-catalog.lisp` | Deterministic catalog compiler |
| `fetch-covers` | Persistent cover-cache updater |
| `ASSETS.md` | Artwork provenance |

The deployer installs `retro-deck-refresh` as
`/usr/sbin/retro-deck-refresh`. It installs the keyboard quirk helper and
hotplug hook from this directory under `/usr/sbin` and `/etc/hotplug.d/usb`.
It installs `nes-deck-swap.init` as `/etc/init.d/nes-deck-swap`.

The BMC package contains the approved settings icon directly from
`crates/retro-deck-dashboard/native/assets/gear-knekko-09.png`.

## Catalog contract

`games.sexp` is one schema-versioned property list. Every game has exactly
these keys:

1. `:id`, a stable lowercase identifier
2. `:title`, the displayed title
3. `:system`, one of `:nes`, `:gb`, `:gbc`, `:zx`, `:chip8`, or `:deck`
4. `:rom`, an absolute normalized runtime path
5. `:color`, a canonical xterm-256 `#RRGGBB` accent color

Console paths must be below `/mnt/data/roms/<system>/` and use an extension
accepted for that system. Deck programs remain below
`/mnt/data/nes-deck/games/`.

`compile-catalog.lisp` rejects missing, duplicate, and unknown keys; duplicate
IDs and paths; dotted, circular, and oversized input; reader evaluation;
control characters; non-ASCII text; invalid colors; and paths outside the
persistent installation. It writes `games.tsv` and `palette.tsv` atomically
only after the complete source validates.

The native dashboard loads the checked-in catalog plus the uploader-managed
supplement at `/mnt/data/nes-deck/uploads/games.tsv`. An invalid supplement is
ignored without preventing startup. Cover files are decoded from the cache at
`/mnt/data/nes-deck/covers` and fetched only when absent.

## Palette contract

`palette.tsv` contains one full-RGB value for each semantic role. A complete
version-2 or version-3 override at
`/mnt/data/nes-deck/state/dashboard-palette.sexp` may replace it. The retired
version-3 cog choice is ignored.

The native dashboard validates a complete palette before applying it. A
missing or malformed override falls back to the checked-in palette, and a
missing or malformed checked-in palette falls back to compiled defaults.
Appearance data therefore cannot prevent startup.

## Local validation

With ECL available:

```sh
ecl --norc --shell compile-catalog.lisp games.sexp \
  /tmp/games.tsv /tmp/palette.tsv /tmp/missing-palette-override.sexp
cmp games.tsv /tmp/games.tsv
cmp palette.tsv /tmp/palette.tsv
```

Run `tests/catalog_test.sh`, `tests/retro_deck_refresh_test.sh`, and
`tests/settings_icons_test.sh` after changing catalog, palette, cover, or icon
behavior.
