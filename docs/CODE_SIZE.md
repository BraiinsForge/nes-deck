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
| `retro-deck-apps` | 4,729 | 16.98% |
| `retro-deck-audio` | 249 | 0.89% |
| `retro-deck-config` | 1,243 | 4.46% |
| `retro-deck-dashboard` | 3,685 | 13.23% |
| `retro-deck-emulator` | 6,231 | 22.38% |
| `retro-deck-platform` | 3,934 | 14.13% |
| `retro-deck-policy` | 1,521 | 5.46% |
| `retro-deck-ui` | 570 | 2.05% |
| `retro-deck-uploader` | 4,962 | 17.82% |
| Common Lisp policy | 721 | 2.59% |
| **Total** | **27,845** | **100.00%** |

The emulator row contains a 99-line first-party C adapter around c-octo. By
language, the total is 27,025 Rust lines (97.06%), 721 Common Lisp lines
(2.59%), and 99 C lines (0.36%). There is no first-party Go or C++ source.

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
