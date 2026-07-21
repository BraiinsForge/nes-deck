# First-party code size

This is the review baseline for the BMC-backed Rust and Common Lisp
implementation. It measures code lines, not physical lines, and deliberately
counts inline tests. Generated files, pinned dependencies, `terminal/fbterm`,
Nix, web assets, deployment scripts, and the C++ rollback dashboard are outside
this metric.

Run the aggregate measurement with Tokei 14:

```sh
tokei crates lisp
```

## Selected Retro Deck code

| Component | Code lines | Share |
| --- | ---: | ---: |
| `retro-deck-apps` | 4,729 | 16.85% |
| `retro-deck-audio` | 249 | 0.89% |
| `retro-deck-config` | 1,500 | 5.35% |
| `retro-deck-dashboard` | 3,685 | 13.13% |
| `retro-deck-emulator` | 6,235 | 22.22% |
| `retro-deck-platform` | 3,934 | 14.02% |
| `retro-deck-policy` | 1,521 | 5.42% |
| `retro-deck-ui` | 570 | 2.03% |
| `retro-deck-uploader` | 4,962 | 17.69% |
| Common Lisp policy | 672 | 2.40% |
| **Total** | **28,057** | **100.00%** |

The emulator row contains a 99-line first-party C adapter around c-octo. By
language, the total is 27,286 Rust lines (97.25%), 672 Common Lisp lines
(2.40%), and 99 C lines (0.35%). There is no selected first-party Go or C++.

The standalone uploader remains because it already uses established Axum,
Serde, and Tokio libraries rather than implementing an HTTP stack. Its future
authentication integration with BMC is separable from the native dashboard,
emulator, input, audio, and policy rewrite.

## Boundary checks

- `vendor/emulators/**` and `terminal/fbterm/**` are marked
  `linguist-vendored` in `.gitattributes`.
- Relative to `migration/rust-lisp`, this branch adds 6,871 and deletes 23,276
  Rust, Lisp, C, header, and C++ source lines: a net reduction of 16,405.
- The general BMC feature branch adds 4,086 and deletes 754 Rust source lines
  relative to its upstream base: net 3,332. Those lines implement reusable BMC
  widget input, application supervision, and audio facilities and are not
  counted as Retro Deck code above.
