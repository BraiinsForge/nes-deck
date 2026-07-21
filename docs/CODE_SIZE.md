# First-party code size

This is the review baseline for the BMC-backed Rust and Common Lisp
implementation. It measures code lines, not physical lines, and deliberately
counts inline tests. Generated files, pinned dependencies, `terminal/fbterm`,
Nix, web assets, and deployment scripts are outside this metric.

Run the aggregate measurement with Tokei 14:

```sh
tokei crates lisp
```

## Selected Retro Deck code

| Component | Code lines | Share |
| --- | ---: | ---: |
| `retro-deck-apps` | 4,729 | 16.95% |
| `retro-deck-audio` | 249 | 0.89% |
| `retro-deck-config` | 1,292 | 4.63% |
| `retro-deck-dashboard` | 3,685 | 13.21% |
| `retro-deck-emulator` | 6,235 | 22.35% |
| `retro-deck-platform` | 3,934 | 14.10% |
| `retro-deck-policy` | 1,521 | 5.45% |
| `retro-deck-ui` | 570 | 2.04% |
| `retro-deck-uploader` | 4,962 | 17.79% |
| Common Lisp policy | 721 | 2.58% |
| **Total** | **27,898** | **100.00%** |

The emulator row contains a 99-line first-party C adapter around c-octo. By
language, the total is 27,078 Rust lines (97.06%), 721 Common Lisp lines
(2.58%), and 99 C lines (0.35%). There is no first-party Go or C++ source.

The standalone uploader remains because it already uses established Axum,
Serde, and Tokio libraries rather than implementing an HTTP stack. Its future
authentication integration with BMC is separable from the native dashboard,
emulator, input, audio, and policy rewrite.

## Boundary checks

- `vendor/emulators/**` and `terminal/fbterm/**` are marked
  `linguist-vendored` in `.gitattributes`.
- BMC sources are pinned Git dependencies and are not copied into this
  repository or counted as Retro Deck code.
- `tests/vendor_emulators_test.sh` verifies complete emulator source, patch,
  license, and hash manifests.
