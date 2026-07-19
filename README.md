# Retro Deck

Retro Deck turns a Braiins Forge Deck into a persistent touch-first game and
program launcher. It runs as a fullscreen BMC scene when `bmc-compositor` is
installed and retains a direct-framebuffer mode for Decks without BMC. The
dashboard provides NES, Game Boy, Game Boy Color, ZX Spectrum, CHIP-8,
utilities, language REPLs, and a chiptune player. Two Retro Games THEGamepad
controllers work as stable Player 1 and Player 2 devices.

## Deploy to a Deck

You need:

- a Braiins Forge Deck reachable as `root` over SSH
- a mounted `/mnt/data` partition on the Deck
- a Linux development machine with Nix flakes, an OpenSSH client, tar, gzip,
  standard GNU utilities, and `xxd`
- this private repository, including the owner-supplied `roms/` library
- stable power during activation

Clone the repository, create its private per-Deck configuration, validate the
fresh-install plan, and provision:

```sh
git clone git@github.com:BraiinsForge/retrodeck.git
cd retrodeck
install -d -m 0700 ~/.config/retro-deck/decks
./ops/configure-deck.sh ~/.config/retro-deck/decks/my-deck.conf
./ops/provision-deck.sh --config ~/.config/retro-deck/decks/my-deck.conf --check
./ops/provision-deck.sh --config ~/.config/retro-deck/decks/my-deck.conf
```

The setup command asks for the Deck's current SSH address, its unique
WireGuard address, routed prefix, health-check address, and ROM uploader
password. It writes a mode-`0600` file below the operator's config directory,
outside the Git checkout. For multiple Decks, create a private directory and
pass one explicit file per Deck as the setup command's positional path, then
use `--config PATH` with the provision, deployment, and health-check commands.

WireGuard topology is also private operator state. Put a mode-`0600` client
`setconf` file at `~/.config/retro-deck/wireguard/wg0.conf`; it must contain
the public server peer and an `AllowedIPs` value equal to the routed prefix,
but no Deck private key. Put an executable peer-registration command at
`~/.config/retro-deck/wireguard/register-peer`. The provisioner invokes it as
`register-peer ADDRESS/32 PUBLIC_KEY`; use `--skip-peer-registration` only
when that peer is already configured by another system. Paths can be replaced
with `--wireguard-config` and `--register-peer-command`.

The fresh-Deck provisioner defaults to the development machine's IWD profiles
in `/var/lib/iwd`; override that with `--wifi-profiles`. It imports only
regular `.psk` files; open and enterprise
profiles are counted and deliberately ignored. Up to seven recently modified
personal profiles seed the fast-failover order, with `BraiinsRecovery` kept as
the final insurance entry when present. Verify the fresh Deck's SSH host key
before running it. The script keeps normal SSH host-key checking enabled.

Provisioning snapshots `/etc/config/wireless`, the current `wlan0` address,
and the complete default route before changing anything. It installs the
guarded profile selector without reloading Wi-Fi, preserves a private
WireGuard key already present on the Deck, delegates peer collision handling
to the external registrar, and checks that the Wi-Fi snapshot is byte-for-byte
unchanged before invoking the application deployer. `--network-only` performs
just this idempotent network preparation.

The first build downloads the pinned ARM toolchain and can take several
minutes. The script builds every static runtime, verifies the staged payload,
uploads it below `/mnt/data`, briefly stops the dashboard, activates the new
files, installs the configured uploader credential, and waits for `deck-menu`
and the ROM uploader to be ready. If activation fails after a
service is stopped, the script attempts to restart it before exiting.

The deployment script does not edit, reload, or disconnect Wi-Fi. It merges
the tracked ROMs and CC0 chiptunes into persistent storage without deleting
additional files, save games, or user programs already on the Deck.
On a fresh Deck, its readiness check allows the bounded first Libretro cover
cache fill to finish. Large cover indexes are decoded in one process rather
than spawning a process per filename.

Verify the result:

```sh
./ops/check-deck.sh --config ~/.config/retro-deck/decks/my-deck.conf
```

The health check is read-only. It understands both BMC compositor and direct
framebuffer installations, reports processes and network identity, prints the
last 20 dashboard log lines, and exits unsuccessfully when a required component
is missing.

To update an already-provisioned Deck, pull the repository and run
`./ops/deploy.sh --config PATH`. A positional `root@DECK-IP` temporarily
overrides that configuration's SSH target.

If Nix is not installed, follow the
[official installation instructions](https://nixos.org/download/). Detailed
build and test commands are in [BUILD.md](BUILD.md).

## Included systems and programs

| Dashboard section | Runtime | Persistent content |
| --- | --- | --- |
| NES | FCEUmm | `/mnt/data/roms/nes` |
| Game Boy | Gambatte | `/mnt/data/roms/gb` |
| Game Boy Color | Gambatte | `/mnt/data/roms/gbc` |
| ZX Spectrum | Fuse | `/mnt/data/roms/zx` |
| CHIP-8 | c-octo | `/mnt/data/roms/chip8` |
| Deck | Native programs and REPLs | `/mnt/data/langs`, `/mnt/data/chiptunes` |

The Deck section contains:

- 10 Seconds, a native touch and controller timing game
- a `/bin/ash` framebuffer terminal with US ANSI and Czech QWERTZ layouts
- Lua 5.5, rlwrap-backed ECL Common Lisp 26.5.5, MicroPython 1.25, and
  Chibi Scheme 0.11
- a native GME and Ogg Vorbis chiptune player
- a guarded reboot action

REPL files persist under `/mnt/data/langs/{lua,lisp,python,scheme}`. The music
player scans `/mnt/data/chiptunes` for `ay`, `gbs`, `gym`, `hes`, `kss`, `nsf`,
`nsfe`, `ogg`, `sap`, `spc`, `vgm`, and `vgz` files. Ogg files must be 44.1 kHz
mono or stereo. Ten CC0 tracks are included with provenance and checksums in
[chiptunes/README.md](chiptunes/README.md).

## Upload ROMs over Wi-Fi or WireGuard

Open `http://<DECK_WLAN_ADDRESS>:8080` on the Deck's local network or
`http://<DECK_WIREGUARD_ADDRESS>:8080` through WireGuard, then sign in with the
password from your private deployment configuration. The
Paper-style intake page accepts a raw NES, GB, GBC, ZX Spectrum, or CHIP-8 ROM,
or a ZIP containing exactly one matching ROM. It validates the payload,
refuses to replace an existing file, files it below
`/mnt/data/roms/<system>/`, updates a private supplemental catalog, and
restarts the dashboard so the game appears.

The service listens on every IPv4 interface. It never changes Wi-Fi,
WireGuard, routes, or firewall state. Authentication, CSRF, upload limits, and
password rotation are documented in
[deploy/uploader/README.md](deploy/uploader/README.md).

## Using the dashboard

Tap a system tab or use either controller's shoulder buttons to change
sections. Tap a visible card, or select it with Left/Right and press A, to
launch it. The small hollow rectangles show the selected position and total
number of entries.

A connected keyboard uses the arrow keys to move, Enter to activate, and
Escape to go back. Tab changes to the next console and Shift-Tab changes to the
previous console, matching the controller's R/L shoulders. The dashboard grabs
keyboard input only while it is visible and releases it before starting a game
or terminal.

The gear or controller Select opens settings. Its controls adjust volume and
backlight brightness, open the terminal, switch terminal keymaps, and add a
Wi-Fi profile. Volume and brightness persist below `/mnt/data`; volume uses
five-point steps and brightness is bounded from 10 through 100. Menu actions
play short cues while sound is enabled. The service disables console blanking
at boot and whenever a child program returns.

The small `(c)` control in the bottom-left opens the animated FOSS dependency
and license crawl. B or the top-right close control returns to the dashboard.
Every line uses one source text size projected continuously onto a receding
plane, with a static starfield and a fade near the horizon. The crawl is
generated from the tracked `deploy/menu/credits.tsv` manifest; complete
installed license texts are kept under `/mnt/data/nes-deck/licenses`.
Set `RETRO_DECK_REDUCED_MOTION=1` in the dashboard environment to replace the
crawl with a static project/license sheet and disable animation wakeups.

The Common Lisp REPL runs through `rlwrap`; editable command history persists
privately as `/mnt/data/langs/lisp/.ecl_history`.

Console emulators show an outlined cross in the top-left corner. Hold it for
two seconds to return to the dashboard. A two-second hold anywhere also leaves
a running emulator or terminal, and touch does not emulate game controls. In
the chiptune player, the top-right cross returns immediately. The four bottom
icons control playback mode, previous file, play/pause, and next file.
Controller Left/Right also changes files, Up/Down changes the persistent volume
in five-point steps, L/R changes subsongs when a music file exposes more than
one, A pauses, Start changes playback mode, and B returns.

The Wi-Fi editor only writes a root-only profile. Saving does not scan, roam,
reload networking, or disturb the current connection. The profile becomes
eligible when the current connection is later lost. Network and recovery
findings for the audited unit are in [DECK_NOTES.md](DECK_NOTES.md).

## Controllers

Identical THEGamepad devices are ordered by physical USB path. Keep them in the
same hub ports to preserve Player 1 and Player 2 across boots.

| Console control | THEGamepad |
| --- | --- |
| D-pad | D-pad |
| A | A or X |
| B | B or Y |
| Start | Start |
| Select | Back |

NES, GB, and GBC use this mapping. CHIP-8 uses the standard Octo mapping,
except Space Racer maps one controller to each ship. ZX Spectrum assigns
Kempston to Player 1 and Sinclair 2 to Player 2; A/X fires, Back opens the
Spectrum keyboard, L is Enter, and R is Space. A connected physical keyboard
is passed through as the Spectrum keyboard, so letters, digits, Space, Enter,
Backspace, Shift, Control, Alt, and the arrow keys retain their keyboard
meaning instead of using the console-game mapping below.

A keyboard remains a Player 1 fallback for NES, GB/GBC, and CHIP-8:

| Console control | Keyboard |
| --- | --- |
| D-pad | Arrow keys or WASD |
| A | Space; Z or J also work |
| B | Shift; X or K also work |
| Start | Enter |
| Select | Control |

## ROMs and save games

`roms/<system>/` is the canonical tracked library. Supported intake folders
are `nes`, `gb`, `gbc`, `zx`, and `chip8`. A ROM or single-ROM ZIP at the
repository root is unprocessed intake and must be validated, renamed, filed,
checksummed, and added to the catalog before deployment. See
[roms/README.md](roms/README.md) for the exact intake contract.

The repository contains owner-supplied console ROMs and only freely licensed
CHIP-8 ROMs. The reproducible CHIP-8 sources, licenses, and hashes are recorded
in [FOSS_GAMES.md](FOSS_GAMES.md).

NES battery SRAM is saved atomically beside the ROM as `.srm`. GB and GBC use
`.sav` plus `.rtc` when the cartridge has a real-time clock. The deploy script
preserves these sidecars. ZX TAP files are read-only tape media and do not
produce automatic save files.

## Operations and recovery

Check the service and its bounded persistent log:

```sh
./ops/check-deck.sh --config ~/.config/retro-deck/decks/my-deck.conf
```

Restart the dashboard without rebooting the Deck:

```sh
ssh root@DECK-IP '
  if [ -x /etc/init.d/bmc-compositor ]; then
    /etc/init.d/bmc-compositor restart
  else
    /etc/init.d/nes-deck restart
  fi'
```

If the display remains black, inspect the log before changing files or network
configuration. BMC owns the dashboard process when its compositor is present;
the `nes-deck` service owns the direct-framebuffer installation. Both require
`/mnt/data` to be mounted. The direct-framebuffer path also validates display
geometry, hides the console cursor, and unblanks the panel on every return.

## Development

- [BUILD.md](BUILD.md) covers reproducible builds, tests, screenshots, and
  platform details.
- [docs/swipe-rendering-postmortem.org](docs/swipe-rendering-postmortem.org)
  explains the buffer-lifecycle and deployment failures behind the clock
  swipe rendering incident.
- [deploy/menu/README.md](deploy/menu/README.md) defines the installed layout
  and strict catalog schema.
- [DECK_NOTES.md](DECK_NOTES.md) records verified hardware, audio, display,
  Wi-Fi, WireGuard, and recovery behavior.
- [AGENTS.md](AGENTS.md) defines repository-specific ROM handling.

## License

The project combines components under GPL, LGPL, BSD, MIT, and CC0 terms.
Exact upstream revisions are pinned, and required license texts are installed
with the binaries. The tracked summary and archive construction are documented
in [THIRD_PARTY.md](THIRD_PARTY.md). Owner-supplied ROMs remain private and are
not relicensed.
