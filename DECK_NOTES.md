# Braiins Deck software notes

These notes describe the inspected `stm32mp157c-ii3-bmc1` Deck and the
deployment used by this repository. They intentionally contain no Wi-Fi
passphrases, WireGuard private keys, or ROM data.

## Platform and storage

- OpenWrt 22.03.4 (`stm32mp15/ii3`), Linux 5.10.176, procd init.
- Dual-core ARMv7 Cortex-A7, hard-float NEON/VFPv4, musl 1.2.3.
- About 246 MiB RAM and no swap.
- The writable OpenWrt overlay is only about 116 MiB. Persistent application
  data belongs on the 2.8 GiB ext4 partition mounted at `/mnt/data` by
  `/etc/init.d/pre-exec-mount` (START=90).
- This deployment uses `/mnt/data/nes-deck` for the emulator, ECL runtime,
  touchscreen menu, ROMs, licenses, catalog, logs, and persistent state. The
  small procd launcher remains in `/etc/init.d/nes-deck`.

## Display

- `/dev/fb0` is `stmdrmfb` for a Sitronix ST7703 DSI panel.
- The physical framebuffer is 600x1280, RGB565 little-endian, with a 1280-byte
  pitch and a 1,638,400-byte mapping. Each line therefore has 80 padding bytes;
  using `xres * bytes_per_pixel` as the pitch corrupts rendering.
- The panel is physically portrait and must be rotated in software for the
  Deck's landscape orientation. The framebuffer/DRM rotation properties are
  identity, not a hardware 90-degree transform.
- The kernel boots with `consoleblank=600`. A framebuffer owner can keep
  running normally while console scanout goes black after ten minutes. The
  menu launcher now sends the Linux console disable/unblank sequences on every
  service start, and the native menu issues `FBIOBLANK` with
  `FB_BLANK_UNBLANK` whenever it opens fb0. This also restores the panel after
  games and the terminal release the framebuffer.
- InfoNES renders the 256x240 frame at integer 2x scale. Its 512x480 output is
  centered at logical x=384..895 and fills the active y=0..479 panel. Earlier
  offsets shifted it 25 pixels left, left a 40-pixel gap above it, and clipped
  the final 20 NES scanlines.
- A 2026-07-13 Mario benchmark found that the original per-pixel mapped writes
  consumed 10.8 to 11.0 ms per frame, with a 14.9 ms observed maximum, despite
  audio pacing holding roughly 60 FPS. Building a complete 491,520-byte frame
  in cacheable RAM and publishing contiguous rows reduced the same path to
  4.7 ms average and 7.4 ms maximum. The `stmdrmfb` driver implements
  `FBIO_WAITFORVSYNC`, but synchronous use raised the average to roughly
  11.8 ms and introduced 20 to 23 ms phase-miss spikes. It is therefore an
  opt-in `INFONES_VSYNC=1` diagnostic; fast staged publication is the default.
  A follow-up long Mario probe captured distinct framebuffer hashes at 3, 20,
  and 40 seconds, confirming changing gameplay/demo frames. Every complete
  120-frame window stayed between 59.73 and 60.53 FPS while render averages
  remained between 4.41 and 4.77 ms.

## Touchscreen

- The `Goodix Capacitive TouchScreen` reports the intended landscape
  coordinates directly: ABS_X 0..1279, ABS_Y 0..479, and BTN_TOUCH, plus
  multitouch slots. No rotation or calibration is required. Event node numbers
  are not stable: with a USB gamepad attached on 2026-07-13, THEGamepad was
  `/dev/input/event0` and Goodix moved to `/dev/input/event1`. The menu discovers
  Goodix by name and capabilities instead of assuming an event number.
- The S99 menu exclusively grabs the Goodix event device while it is active.
  It releases the framebuffer before starting InfoNES but retains touch input
  so a continuous two-second hold anywhere on the touchscreen can terminate
  the game and return to the menu. Touch is not otherwise used during gameplay.
- The menu maps its 1280x480 logical canvas onto the portrait framebuffer as
  `column = y`, `row = 1279 - x`. Its host test verifies every logical pixel
  maps in bounds and exactly once.

## Controllers

- The deployed emulator discovers up to two Retro Games THEGamepad controllers
  with USB ID `1c59:0026` through evdev. Controllers are ordered by physical USB
  path so leaving them in the same Deck or hub ports keeps Player 1 and Player 2
  assignments stable across game launches; keyboard input remains a Player 1
  fallback.
- Retro Games' published mapping is used: D-pad axes are NES directions, A/X
  are NES A, B/Y are NES B, Back is Select, and Start is Start. Disconnects,
  reconnects, hotplug, dropped-event resynchronization, and independent P1/P2
  state are covered by the host test. Setting `INFONES_INPUT_DIAGNOSTICS=1`
  prints state changes for an explicit hardware audit without enabling routine
  input logging.
- On 2026-07-13 the current path-sorted build assigned
  `usb-49000000.usb-otg-1.1/input0` to Player 1 and
  `usb-49000000.usb-otg-1.3/input0` to Player 2. An earlier diagnostic before
  stable path ordering had observed the reverse assignment.
  Each independently produced the exact released sequence Right `0x80`, A
  `0x01`, B `0x02`, Start `0x08`, with state returning to `0x00` after every
  press. The user also started Mario and played successfully with Player 1.
  Diagnostic mode was then stopped, and the normal menu was verified running
  again with both controllers still attached.

## Audio

- ALSA card 0 is `BMC100-MAX98357A-Audio`: STM32 I2S driving a MAX98357A.
- Native playback is stereo S16_LE/S32_LE at 8-96 kHz. `/dev/dsp` is ALSA's
  OSS compatibility layer and accepts S16_LE mono at exactly 44.1 kHz,
  converting it to the hardware's S16_LE stereo layout.
- The old emulator requested only four 128-byte fragments (clamped to 1024
  bytes total) while writing one frame of audio at a time. The deployed stream
  requests eight 1024-byte S16 periods (8192 bytes total, about 93 ms), leaving
  scheduling cushion for framebuffer updates.
- The pinned InfoNES code also indexed a 24-byte `APU_Reg` array with the full
  address `0x4015`, corrupting memory from the core and mapper-0 IRQ paths.
  The repository patches use the correct register offset `0x15`, repair the
  APU frame/envelope counters, replace the incorrect 6/10-bit slow noise
  generator with the NES's 15-bit LFSR, retain every LFSR clock at the fast
  noise periods, correct noise note lengths and the DPCM clock, and timestamp
  register writes in frame cycles instead of collapsing them onto sample zero.
- Mixing uses integer nonlinear NES pulse/TND lookup tables and a continuous
  DC blocker. The emulator now sends signed 16-bit PCM directly, avoiding the
  few-dozen-level effective resolution of the previous U8 stream. PCM gain is
  about 42% of that old path because the fixed-gain MAX98357A is loud on the
  Deck speaker. The launcher exposes this as `VOLUME_PERCENT=42` (valid range
  0-100) instead of relying on ALSA softvol.
- The menu's top-right minus/display/plus group persists a canonical 0-100
  value in `/mnt/data/nes-deck/state/menu-volume.state`. Adjustments move in
  5-point steps. The display is a mute toggle: it is green and shows the
  percentage while audible, then turns red and reads `VOL OFF` when muted.
  Tapping the display or plus restores the last audible level, or the configured
  default when the launcher started muted. Each nonzero adjustment plays a
  two-note confirmation through `/dev/dsp`. The selected value is passed to
  every emulator and survives service restarts and reboot. The launcher
  migrates the former exact `on\n`/`off\n` state to 42/0 once.
- On 2026-07-13 a reported Mario-muted launch was traced to the persisted sound
  state being `off\n`: logd showed the preceding emulator launch opening at 42%
  and the later launch opening at exactly 0%. Mario's title and attract demo are
  also intentionally silent until Start; this was not evidence of a new APU
  regression.
- The deployment reset the persisted state to `on\n`; the subsequent Mario
  launch was logged at 42%, ALSA was RUNNING at 44100 Hz, and the user confirmed
  that Mario gameplay audio worked. The Deck was 95% idle after the temporary
  SSH framebuffer and audio sampling ended.
- Gambatte produces 32768 Hz audio. The OSS compatibility ioctl echoed that
  requested rate even though `/proc/asound/.../hw_params` showed the live
  hardware stream at exactly 32000 Hz. One OSS application frame was consumed
  per hardware frame, slowing emulation by `32768/32000`. The shared runtime
  now requests 32000 Hz explicitly and resamples with one fixed-point step per
  callback instead of a 64-bit division per output sample. Muted Gambatte
  diagnostics held the native 59.728 FPS at 1.004-1.005 seconds per 60 frames;
  the final active-audio probe held approximately 59.4 FPS without recurring
  XRUN noise, and the user confirmed Adjustris no longer produced distorted
  audio.
- The deployed staged-video/S16/two-gamepad build has SHA-256
  `9d2bed939d8a8e44219f7d61c2be9f8a493e23169aa8abe61ec004d3f83907f3`.
  Its live ALSA stream was verified as RUNNING at exact 44100 Hz with 512-frame
  hardware periods and a 4096-frame hardware buffer. The OSS ring is filled
  while its trigger is paused and playback begins on the first callback. During
  65 seconds of checks, every fresh underrun counter stayed within the
  4096-frame ring, playback remained RUNNING, the trigger time stayed fixed,
  and queued audio did not drain. A later read during continued remote
  diagnostics caught one XRUN which recovered automatically on the next write,
  so the stream is not claimed to be mathematically XRUN-proof. The user judged
  gameplay audio "perfect" at 42%. The audio-identical build from immediately
  before the centering fix is saved as
  `/mnt/data/nes-deck/infones.pre-centering-b90b3857`; the preceding S16 build
  is `infones.pre-noise-buffer42-b3e51b0d`, and older builds remain alongside
  them for rollback. The immediately preceding deployed build is also in the
  timestamped
  `/mnt/data/nes-deck/backups/20260713-095726-two-gamepads-title-only/`
  deployment backup.
- Super Mario Bros. deliberately disables all APU channels on its title and
  attract demo; silence there is expected. The inspected Deck had only the
  Goodix touchscreen attached, and touch is not mapped to an NES controller,
  so the moving attract demo was initially mistaken for gameplay. A live
  diagnostic injected a brief Start edge into the running emulator: World 1-1
  then produced changing pulse, triangle, and mixed PCM samples, with ALSA
  RUNNING at 44100 Hz and audio queued. Attach a keyboard/controller and press
  Start/Enter before evaluating Mario's gameplay audio.
- `/etc/asound.conf` defines the `softvol` default. `/etc/init.d/audio-init`
  primes it at boot. Kernel `/dev/dsp` does not reliably pass through that
  userspace softvol, so emulator gain is set in the PCM mixer itself.

## Services and boot order

- The stock UI daemon is `/usr/bin/bmc`, managed by `/etc/init.d/bmc` at S99
  with unlimited respawn. For the emulator deployment it is stopped and
  disabled.
- `/etc/init.d/bmc-video` is a separate synchronous S98 boot animation. It
  exits before the S99 emulator launcher and is not the long-running BMC
  service.
- `/etc/init.d/nes-deck` is a procd service at S99. It starts the persistent
  `/mnt/data/nes-deck/menu/deck-menu-launcher` and uses bounded crash
  respawning. The launcher validates the source catalog with ECL and then
  execs the native menu. Launcher milestones go to logd; catalog output plus
  native menu, emulator, terminal, exit-status, and signal details append to
  the bounded persistent `/mnt/data/nes-deck/log/deck-menu.log`. The previous direct-Mario init script is
  retained as `/etc/init.d/nes-deck.pre-touch-menu-20260711` for rollback.
- `pre-exec-runner` is unrelated to internal storage: it looks for a signed
  script on USB media and is not used by this deployment.

## Menu, ECL, and game catalog

- ECL 26.5.5 is installed under `/mnt/data/nes-deck/ecl`, with a relocatable
  wrapper at `/usr/bin/ecl`. `ecl.bin` is a stripped, statically linked ARMv7
  hard-float executable with no Nix runtime references. Its SHA-256 is
  `ff41f39cf00da7e078c2ea09ef7a510c75fda53fb3c771c1f4f40774e30ff11e`;
  `help.doc` is
  `f80d7c10da0e0a09bde089c8e9ad650701befa14a76f1fc740ddae036dacd536`.
- The static ARM native menu is
  `/mnt/data/nes-deck/menu/deck-menu`, SHA-256
  `73df69b960c9b93f7ee87b821ec87cdd8aa57c0689935da2f4a5797f06e7a2e7`.
  It validates the manifest and system-specific NES/GB/GBC/CHIP-8 game data
  before opening the framebuffer, supervises one emulator child, logs its
  exact exit status or signal, and restores tty state after the child exits.
  This build includes the full-screen two-second return hold, persistent
  volume controls, a persistent US/Czech terminal keymap toggle, the Wi-Fi
  editor, and a supervised framebuffer-terminal action. The installed launcher
  SHA-256 is
  `b210fcff0c3be84fa6845f18653bfe5457c16ae83b090c3b38641888758db0e9`.
- The renderer uses only canonical xterm-256 colors. Five flat tabs labeled
  `NES`, `GAME BOY`, `GAME BOY COLOR`, `CHIP-8`, and `DECK` filter the catalog;
  the selected tab is cool xterm blue-green 109, distinct from warm title 229.
  Every game name is centered at the same compact bitmap-font scale. The
  oversized title is `RETRO DECK`; the redundant play instruction, old blue
  divider, and tile, tab, and top-control outer outlines are gone. No
  description or license text remains in the launcher. Full 1280x480 captures
  of all five tabs, mute and keymap states, four Wi-Fi editor states, and the
  live timer result screen are in `/root/retro-deck-screens` on the deployment
  host.
- Menu transitions build the complete rotated frame in cacheable memory before
  publishing finished rows to live scanout. This removes the visible black
  clear between screens and reduces live framebuffer writes per transition
  from about 2.76 MB to 1.23 MB.
- `games.sexp` is the editable schema-checked source. Schema version 3 keeps
  id, title, system, ROM path, and card color; game cards render only the
  centered title, while redistribution details remain in `FOSS_GAMES.md` and
  installed license files. At each service start,
  the ECL compiler atomically generates
  `/mnt/data/nes-deck/state/games.tsv`; the checked-in TSV is a known-good
  fallback. The actual Deck ECL output was verified byte-for-byte against that
  fallback for all fourteen entries. The deployed `games.sexp` SHA-256 is
  `f40e93cd78fde396d31bb79f5e9b8074814f54b9386a3d62abb70d4100adbc4d`;
  the generated and fallback TSV SHA-256 is
  `6eef36e8687231cf86425f6a0e6d3d9115543ae41e956f2527badb3a1603a4f6`.
  The installed compiler SHA-256 is
  `eeb68d83d8fddf0b9e2996f8c6005d4b91e816a9fb57b75b7cbb8253c4d4d44e`;
  both it and the native loader reject off-palette colors.
- The only freely licensed games retained in the menu are the CHIP-8 titles
  Outlaw and Space Racer. Their provenance, license, and ROM hashes are in
  [FOSS_GAMES.md](FOSS_GAMES.md). The NES, GB, and GBC menus use the owner's
  locally supplied library without claiming redistribution rights. Canonical
  repository paths and hashes are recorded under [`roms/`](roms/README.md).

## GB, GBC, and CHIP-8 emulators

- `/mnt/data/nes-deck/gb-deck` statically hosts the pinned Gambatte libretro
  core at `dfc165599f3f1068c40a0b7ad6fe5f161283d483` for both GB and GBC. It is
  GPL-2.0-only, built with Cortex-A7/NEON tuning and LTO, and emits native
  RGB565 frames. Its deployed SHA-256 is
  `21cdda99e34d8d6747bdcb11e15f71d7a9658582bc3c85705e2b0ea7c2f9bfd8`.
  SRAM and RTC data are saved beside the ROM as `.sav` and `.rtc` files.
- `/mnt/data/nes-deck/chip8-deck` statically hosts the pinned c-octo core for
  CHIP-8, SCHIP, and XO-CHIP. Its deployed SHA-256 is
  `8fa537720f4f496479cf7c8ca98353ac126bc8a26e4bee19d47709c2c830df9b`.
  ROM sidecars hold tickrate, palette, quirk, and controller-profile settings.
- Both frontends share exact framebuffer validation, nearest-neighbor integer
  scaling inside a 16-pixel rounded-panel safe area, OSS S16 mono audio,
  volume inheritance, frame pacing, and the stable two-controller discovery
  code. Completed frames are built in cacheable RAM and only the active
  rotated rectangle is published to the live scanout, preventing the moving
  black triangle caused by partial framebuffer writes. The RGB565 path writes
  one complete physical row at a time. GB's 160x144 image renders at 3x;
  64x32 CHIP-8 renders at 14x, and 128x64 high-resolution modes render at 7x.
- The first hardware smoke run exposed a frontend-only ROM-read bug: a C++
  stream-buffer iterator does not set `ifstream::eof()`, so complete ROMs were
  rejected. The loaders now perform exact-size reads and reject only short or
  bad reads. After that fix, Adjustris, Geometrix, Outlaw, and Space Racer each
  ran on the Deck, rendered a distinct framebuffer, detected Player 1 at
  `usb-49000000.usb-otg-1.1/input0` and Player 2 at
  `usb-49000000.usb-otg-1.3/input0`, and shut down on SIGTERM with status 0.
  The muted probes still opened and exercised `/dev/dsp`; their logs and raw
  framebuffer captures are under
  `/mnt/data/nes-deck/log/live-smoke-20260713-120753-readfix/`.
- SameBoy v1.0.3 could produce only about 32 FPS on the dual Cortex-A7 Deck;
  Cortex-A7/NEON/LTO tuning improved it only to about 35 FPS, and suppressing
  video callbacks did not materially change that result. The core was replaced
  with Gambatte rather than hiding the deficit with frameskip. Final live
  Adjustris and Geometrix probes rendered distinct framebuffer hashes and ran
  near the native 59.728 FPS with sound enabled.
- c-octo's low-resolution pixels use a 64-byte row pitch. The first wrapper
  passed 128, which displayed only the top half with every other line missing;
  the wrapper and host test now require pitch 64. Final Outlaw and Space Racer
  probes rendered distinct framebuffer hashes, and the user confirmed CHIP-8
  rendering was substantially improved. Minor sprite flicker in Outlaw is
  consistent with the game's own draw/erase behavior.
- The pre-Gambatte GB binary is backed up under
  `/mnt/data/nes-deck/backups/20260713-124317-pre-gambatte/`. The preceding
  CHIP-8 runtime is under
  `/mnt/data/nes-deck/backups/20260713-124625-pre-final-runtime/`.
  Final GB/GBC/CHIP-8 smoke logs are retained under
  `/mnt/data/nes-deck/log/live-smoke-20260713-1246-final/`.

## Framebuffer terminal

- The terminal source is integrated under `terminal/` from the Braiins Forge
  fbterm fork, with exact upstream commits and licenses recorded in
  `terminal/PROVENANCE.md`.
- `/mnt/data/nes-deck/terminal/fbterm` is a statically linked ARMv7 hard-float
  executable, SHA-256
  `25cba8b94e194e412a3a7d5f50cbd208927ac795fc7533e8e501fa1d49f623c0`.
  The bundled DejaVu Sans Mono font SHA-256 is
  `9d9bfebceb1c3f6f4ad383ded568a6926086208f43f7f92f92f7e93a1383fa38`.
- fbterm validates the actual 600x1280 RGB565 framebuffer and its stride. It
  rotates the visible 1280x480 region, then keeps a 16-pixel black safe area on
  all four sides for the panel's rounded corners. The resulting terminal
  viewport is 1248x448.
- A live framebuffer capture verified fixed-width `M`, `i`, `W`, digits, and
  punctuation with no old glyph smear. A bottom-row marker remained inside
  the padded viewport. The terminal launcher uses the active `/dev/tty1` and a
  private fontconfig file; its SHA-256 is
  `78bd1b3679fe90c191161ef2e09fb6d6fcdf9ba408b72b846567a294b3f6a155`.
  A live launch reached the interactive `root@braiins-deck` prompt. Exiting the
  shell or using the two-second touch hold returns to the menu.
- BusyBox `ash` attaches `/dev/null` to a non-interactive background job when
  that job has no explicit stdin redirection. The terminal launcher must keep
  fbterm in the background so it can wait for complete shutdown and restore
  the US keymap. The launcher therefore opens `/dev/tty1` read/write both for
  itself and explicitly on the fbterm command. fbterm validates the descriptor
  with `VT_GETSTATE` instead of a pathname lookup. A live four-second probe
  confirmed the fixed terminal remained running; initialization diagnostics
  now stay on the persistent menu log instead of disappearing onto the VT.
- The terminal package includes static ARM `loadkeys`, SHA-256
  `a4c1f63d21bd95708e868079a54be54c012021f4bc8bc1a44fd7140f8eb3e984`,
  plus self-contained US ANSI and Czech QWERTZ maps from kbd 2.7.1. The menu
  persists exactly `us\n` or `cz\n` in `terminal-keymap.state`. The launcher
  loads that selection before fbterm and restores US after fbterm has fully
  exited, including menu-requested TERM. Both packaged maps passed parse-only
  `loadkeys -p` checks on the live Deck before installation.
- The ROM and terminal deployment backup is
  `/mnt/data/nes-deck/backup-roms-terminal-20260713-1551/`.

## Wi-Fi

- Wi-Fi is a USB Realtek 8821CU device using the `rtw_8821cu`/rtw88 stack and
  `wpad-openssl`.
- The PHY permits only one managed station interface. Creating one enabled UCI
  `wifi-iface` per saved network is therefore unsafe.
- The stock setup has one active STA section (`wireless.@wifi-iface[2]`) and
  two disabled AP sections. Runtime association can be 5 GHz even though stale
  radio UCI fields say 2.4 GHz.
- Boot association can take roughly 2.5 minutes after initial reserved-page
  errors from the Realtek driver. Boot services must not assume Wi-Fi is ready.
- Imported profiles are kept root-only and a selector changes the single STA
  section only after it is disconnected. See the installed selector itself
  for operational details; do not copy credentials into this repository.
- The menu's profile helper is `/usr/sbin/deck-wifi-profile-add`, SHA-256
  `52a2fe71bab65cc77818dd6a51b026581cc92d7e51ace22bcf189b482e0e467b`.
  It receives credentials on stdin, commits a root-only canonical profile
  atomically, and only then removes every older spelling of the same SSID. It
  never reloads wireless, scans, roams, or edits `/etc/config/wireless`, so
  saving cannot interrupt the current association.
- The first deployed selector had no boot grace and began active scans while
  the Realtek driver was still in its ~155-second startup recovery. After both
  the workstation and Deck were rebooted, the Deck was reachable on both LAN
  and WireGuard; the earlier conclusion that Ethernet recovery was required
  was incorrect.
- The installed scripts match the hardened repository copies: a 240-second
  grace, three confirming scans, PSK-only candidate selection, and automatic
  rollback. On 2026-07-13 the existing root-only `net1` profile was verified
  against the passphrase supplied by the user and had `AutoConnect=true`; the
  credential is intentionally not recorded here. The `deck-wifi` watcher was
  then enabled and started without reloading wireless. The live association,
  IPv4 default route, and WireGuard tunnel remained up. The user then disabled
  the original hotspot for a live failover test. After the grace and confirming
  scans, the selector switched to `net1` and reached associated IPv4
  default-route state in 17 seconds; the Deck obtained `10.0.1.6/24`, and
  WireGuard resumed with a fresh bidirectional handshake. All 32 imported
  root-only profiles remain stored, and automatic failover is active only after
  the current association has been lost repeatedly.

## WireGuard

- The factory kernel omits both WireGuard and TUN, and the configured custom
  target package feed is a dead URL. Stock target kmods must not be forced onto
  this kernel: its package ABI is
  `5.10.176-1-c5bfc45a30e47807303e5abc3fd4a4f1` with symbol versioning enabled.
- An exact-config `tun.ko` was built from Braiins `linux-stm` commit
  `2aca87d7aa4707aa42bbbfd2a6868df15d4df916`. Its vermagic matches, all 2392
  comparable CRCs from 64 shipped modules matched, and all 194 imported TUN
  CRCs matched the actual Deck zImage. It loaded normally without force flags.
  Installed module SHA-256:
  `88ecbe37e252ebf55f9f2706b0c91113e5c175d2b4e4d1b797fc44673a8bbe68`.
- The module is `/lib/modules/5.10.176/tun.ko`; `/etc/modules.d/30-tun`
  autoloads it. The userspace payload is under
  `/mnt/data/nes-deck/wireguard`: `wireguard-go` 0.0.20250522 and `wg`
  1.0.20260223, both pinned static ARMv7 builds. Rebuild scripts, checksums,
  and public-only configuration are in `ops/deck-wireguard`.
- `/etc/init.d/deck-wireguard` supervises the tunnel at START=96. Its private
  key is root-only at `/etc/wireguard/wg0.key`; never copy that key into this
  repository or a support archive. The Deck is `10.0.0.10/32` and routes only
  `10.0.0.0/24` through the tunnel with a 25-second keepalive.
- The server peer is persistent in `root@10.0.0.1:/etc/wireguard/wg0.conf` as
  `AllowedIPs = 10.0.0.10/32`. It was applied with `wg syncconf`, not by
  restarting the WireGuard service over the controlling tunnel. After reboot,
  a fresh handshake occurred more than seven minutes after Deck boot;
  bidirectional ping and SSH over `10.0.0.10` were verified again.

## Useful checks

As of 2026-07-13 the supervised tabbed menu is running with one `deck-menu`
process and no unmanaged emulator process. The live generated fourteen-game catalog matches
the fallback, `net1` remains associated with its IPv4 default route, all 32
saved PSK profiles remain present, and WireGuard is reachable. No wireless
reload or UCI edit was used during deployment. The rollback copy for this
multi-system deployment is
`/mnt/data/nes-deck/backups/20260713-115636-pre-multisystem/`; the immediately
preceding menu and emulator binaries are additionally retained in
`20260713-120436-pre-console-tabs/` and
`20260713-120736-pre-rom-read-fix/`. The volume/keymap deployment backup is
`/mnt/data/nes-deck/backup-controls-20260713-1535/`; the restarted service
initialized volume 42 and US terminal keys without changing Wi-Fi.
The current timer/menu/NES deployment rollback is
`/mnt/data/nes-deck/backups/20260713-163052-pre-ten-seconds/`; the menu binary
immediately preceding the simplified header is retained there as
`deck-menu.pre-header`. The xterm palette and mute-toggle deployment rollback is
`/mnt/data/nes-deck/backups/20260713-184842-pre-xterm-mute/`; it also retains the
menu binary immediately preceding unified title sizing as
`deck-menu.pre-unified-titles` and the binary immediately preceding staged menu
transitions as `deck-menu.pre-staged-present`.

```sh
# Stock UI must stay disabled
/etc/init.d/bmc enabled; echo "$?"   # expected nonzero
pgrep -x bmc                         # expected no output

# Touch-menu service
/etc/init.d/nes-deck status
pidof deck-menu
pidof infones
tail -n 100 /mnt/data/nes-deck/log/deck-menu.log
hexdump -C /mnt/data/nes-deck/state/menu-volume.state
hexdump -C /mnt/data/nes-deck/state/terminal-keymap.state

# ECL and generated catalog
/usr/bin/ecl --norc --eval '(format t "~A~%" (lisp-implementation-version))' \
  --eval '(quit)'
cmp /mnt/data/nes-deck/menu/games.tsv \
  /mnt/data/nes-deck/state/games.tsv

# Userspace WireGuard
/etc/init.d/deck-wireguard status
/mnt/data/nes-deck/wireguard/bin/wg show wg0
ip address show dev wg0
ip route show 10.0.0.0/24

# Hardware stream state while playing
cat /proc/asound/card0/pcm0p/sub0/hw_params
cat /proc/asound/card0/pcm0p/sub0/status

# Framebuffer facts
cat /sys/class/graphics/fb0/virtual_size
cat /sys/class/graphics/fb0/stride
cat /sys/class/graphics/fb0/bits_per_pixel
```
