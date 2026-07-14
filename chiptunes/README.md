# Included chiptunes

These tracks seed `/mnt/data/chiptunes` during deployment. The player also
discovers compatible files added directly to that directory later.

All ten included tracks were published under the
[CC0 1.0 Universal public-domain dedication](https://creativecommons.org/publicdomain/zero/1.0/).
Attribution is not required, but the original authors and asset pages are
recorded here for provenance.

| File | Track | Author | Source |
| --- | --- | --- | --- |
| `ch-ay-na.ogg` | CH-AY-NA | Spring Spring | [OpenGameArt](https://opengameart.org/content/ch-ay-na) |
| `charge.ogg` | Charge | Centurion_of_war | [OpenGameArt](https://opengameart.org/content/charge) |
| `chipnese.ogg` | Chipnese | Spring Spring | [OpenGameArt](https://opengameart.org/content/chipnese) |
| `crazy.ogg` | Chiptune Loop - Crazy | MatiasVME | [OpenGameArt](https://opengameart.org/content/chiptune-loop-crazy) |
| `delayed-chiptune.ogg` | Delayed Chiptune | Centurion_of_war | [OpenGameArt](https://opengameart.org/content/delayed-chiptune) |
| `exploration.ogg` | Chiptune - Exploration | ansimuz | [OpenGameArt](https://opengameart.org/content/chiptune-exploration) |
| `going-up.ogg` | Going Up Adventure Chiptune | ansimuz | [OpenGameArt](https://opengameart.org/content/going-up-adventure-chiptune) |
| `on-the-offensive.ogg` | On the Offensive | Wolfgang_ / Ted Kerr | [OpenGameArt](https://opengameart.org/content/8-bit-theme-on-the-offensive) |
| `opening-theme.ogg` | Opening Theme | Spring Spring | [OpenGameArt](https://opengameart.org/content/opening-theme) |
| `overworld-theme.ogg` | Overworld Theme | Louswan | [OpenGameArt](https://opengameart.org/content/overworld-theme-0) |

The files were retrieved on 2026-07-14. `exploration.ogg` and `going-up.ogg`
were resampled from the published 48 kHz Ogg files to the player's supported
44.1 kHz rate without other edits. Verify the library with:

```sh
cd chiptunes && sha256sum -c SHA256SUMS
```
