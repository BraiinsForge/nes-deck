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
- This deployment uses `/mnt/data/nes-deck` for the emulators, ECL runtime,
  touchscreen menu, licenses, catalog, logs, and persistent state. Canonical
  ROMs live under `/mnt/data/roms`. The small procd launcher remains in
  `/etc/init.d/nes-deck`.

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
- The panel backlight is exposed through
  `/sys/class/backlight/display-bl/{brightness,max_brightness}`. The inspected
  Deck reports a maximum of 20. Dashboard settings maps persistent 10-point
  percentages onto that live range and clamps at 10 percent so an accidental
  touch cannot make the screen appear dead.
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
- The dashboard discovers the same two controllers without relying on event
  node numbers. Either controller's L/R shoulders switch console tabs,
  Left/Right changes the selected game, A launches it, and Select opens or
  closes settings. D-pad directions move through settings, A activates the
  selected control, and B closes settings or the Wi-Fi keyboard. Dashboard
  descriptors close before a child starts and are rediscovered afterward, so
  gameplay events never accumulate in the menu and controller ownership starts
  cleanly for each emulator.
- Retro Games' published mapping is used: D-pad axes are NES directions, A/X
  are NES A, B/Y are NES B, Back is Select, and Start is Start. Disconnects,
  reconnects, hotplug, dropped-event resynchronization, and independent P1/P2
  state are covered by the host test. Physical L/R are also exposed to the
  Spectrum frontend during gameplay. Setting
  `INFONES_INPUT_DIAGNOSTICS=1`
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

## Cartridge saves

- Cartridge saving is automatic and uses sidecars beside each canonical ROM.
  The current InfoNES build has SHA-256
  `c75d6d96994faa977953bcba18b4913a6f284d645c3d8bdd3d283dabdd94f27c`.
  For an NES cartridge with the iNES battery flag, it loads `.srm`, checks for
  changed SRAM every 600 frames, and writes through a synced temporary file
  plus atomic rename. It also performs a final save during graceful shutdown.
  Kirby's Adventure is the only NES title currently filed with that flag.
- Gambatte loads `.sav` and `.rtc` memory exposed by the cartridge, then saves
  changed data every 600 frames and at shutdown using the same temporary-file
  replacement pattern. Final Fantasy Legend III, Pokemon Red, Donkey Kong
  Country, and Super Mario Bros. Deluxe have battery-backed RAM. The current
  Kirby's Dream Land cartridge has neither RAM nor a battery.
- The menu sends SIGTERM and gives an emulator four seconds to flush its save
  before escalating. Existing live Pokémon Red, Donkey Kong Country, and Super
  Mario Bros. Deluxe saves were preserved during the ROM layout migration.
  ZX Spectrum TAP images and CHIP-8 titles in this catalog do not expose
  cartridge save memory. Automatic Fuse state restoration was tested and
  removed because a state captured during Elite loading could wedge the core.

## Audio

- `/mnt/data/nes-deck/nes-deck` is the statically linked, Cortex-A7/NEON-tuned
  FCEUmm frontend. The deployed performance build has SHA-256
  `c75d6d96994faa977953bcba18b4913a6f284d645c3d8bdd3d283dabdd94f27c`.
- ALSA card 0 is `BMC100-MAX98357A-Audio`: STM32 I2S driving a MAX98357A.
- Native playback is stereo S16_LE/S32_LE at 8-96 kHz. `/dev/dsp` is ALSA's
  OSS compatibility layer and accepts S16_LE mono at exactly 44.1 kHz,
  converting it to the hardware's S16_LE stereo layout.
- The old emulator requested only four 128-byte fragments (clamped to 1024
  bytes total) while writing one frame of audio at a time. The deployed stream
  requests eight 1024-byte S16 periods (8192 bytes total, about 93 ms), leaving
  scheduling cushion for framebuffer updates.
- FCEUmm produces 48 kHz audio, but the Deck's OSS bridge consumes about
  47,328 mono application frames per second even while both OSS and ALSA report
  a nominal 48 kHz stream. Synchronous writes therefore slowed 60 emulated
  frames from the required 0.998 seconds to a measured 1.012 seconds. The
  shared runtime now resamples to that measured application clock and sends
  blocking writes from a bounded audio worker. A 45-second Kirby probe averaged
  0.998306 seconds per 60 frames with audio enabled, held its software queue at
  786 to 788 frames, and dropped no samples. Follow-up GB and CHIP-8 probes held
  their native 59.728 and 60.000 FPS clocks with zero dropped samples.
  A post-deployment 90-second Mario attract-mode soak then averaged 0.998178
  seconds across 90 consecutive 60-frame windows, stayed between 0.992 and
  1.006 seconds, produced changing framebuffer hashes through the final
  90-second sample, and dropped no audio samples.
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
- The settings volume down/up controls persist a canonical 0-100 value in
  `/mnt/data/nes-deck/state/menu-volume.state`. Adjustments move in 5-point
  steps; down reaches mute and up restores the last audible level, or the
  configured default when the launcher started muted. Each nonzero adjustment
  plays a two-note confirmation through `/dev/dsp`. The selected value is
  passed to every emulator and survives service restarts and reboot. The
  launcher migrates the former exact `on\n`/`off\n` state to 42/0 once.
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
  `c75d6d96994faa977953bcba18b4913a6f284d645c3d8bdd3d283dabdd94f27c`.
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
- Lua 5.5.0 is installed at `/mnt/data/nes-deck/langs/lua`, SHA-256
  `bce111b82c9e8e9f4d77d1dae287cfcdb4bd94c9a4da57ee5e24036482ae628e`.
  It is a stripped static ARMv7 hard-float executable built from the pinned
  official source archive, with no Nix runtime references. Lua and ECL use
  private persistent working directories at `/mnt/data/langs/lua` and
  `/mnt/data/langs/lisp`.
- The static ARM native menu is
  `/mnt/data/nes-deck/menu/deck-menu`, SHA-256
  `e3b1b6f87176a475218b4d73188fa1936b9a2090496333801c1e4cdff676ef89`.
  It validates the manifest and system-specific NES/GB/GBC/ZX/CHIP-8 game data
  before opening the framebuffer, supervises one emulator child, logs its
  exact exit status or signal, and restores tty state after the child exits.
  This build includes the full-screen two-second return hold, persistent volume
  and safe 10-through-100 brightness controls, a persistent US/Czech terminal
  keymap toggle, the Wi-Fi editor, a supervised shell terminal, and supervised
  Lua and Lisp REPLs. The installed launcher SHA-256 is
  `14b5b0056a84f7c03fc93b7734cf065a28f150019276387dbb6314d3c80cb20a`.
  Successful controller and touchscreen navigation uses the same short
  chiptune cues while volume is audible.
- The renderer uses exact `#fe6c27` interface borders and the 30-percent
  `#ffb896` active composite over black; catalog accents and decoded covers stay
  in canonical xterm-256 colors. Every populated console is a cut-corner top
  tab. The horizontal carousel shows at most three square-cropped covers with
  fixed-size truncated names, white outline arrows, a filled selected card,
  and hollow position markers for the complete game count. A pixel gear opens
  a dedicated settings screen with volume, brightness, terminal, keymap, and
  Wi-Fi controls. The terminal, Lua, Lisp, timer, and reboot app marks are drawn
  natively. No product label, description, or license text remains in the
  launcher. Reproducible 1280x480 captures of every game position, settings
  variants, reboot confirmation, and all four Wi-Fi keyboard states are in
  `/root/retro-deck-screens` on the deployment host, together with a contact
  sheet and the reproducibly rendered timer result screen. The current 30-PNG
  set has all nineteen carousel positions, four settings variants, reboot
  confirmation, four Wi-Fi states, the timer, and its contact sheet.
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
  fallback for all fifteen entries. The deployed `games.sexp` SHA-256 is
  `bb28f1de3630df3704336b5d6a89979b6f2d8cfe0e18a725dac7862e14582541`;
  the generated and fallback TSV SHA-256 is
  `e52c2ff6a3b1fb9fc18b822ceef4da9e067855c8b88d24b932d8c1905985245c`.
  The installed compiler SHA-256 is
  `95461e8e93bc82fc7476257babc051fbcd4d50200f50ba583bb0f30548d20f0c`;
  both it and the native loader reject off-palette colors.
- The only freely licensed games retained in the menu are the CHIP-8 titles
  Outlaw and Space Racer. Their provenance, license, and ROM hashes are in
  [FOSS_GAMES.md](FOSS_GAMES.md). The NES, GB, GBC, and ZX menus use the
  owner's locally supplied library without claiming redistribution rights.
  Canonical repository paths and hashes are recorded under
  [`roms/`](roms/README.md).

## GB, GBC, ZX Spectrum, and CHIP-8 emulators

- `/mnt/data/nes-deck/gb-deck` statically hosts the pinned Gambatte libretro
  core at `dfc165599f3f1068c40a0b7ad6fe5f161283d483` for both GB and GBC. It is
  GPL-2.0-only, built with Cortex-A7/NEON tuning and LTO, and emits native
  RGB565 frames. Its deployed SHA-256 is
  `1b81dfa37afe3cbb861c9be742ef7a9e6d097f54a3e844b0719a3768665c09a3`.
  SRAM and RTC data are saved beside the ROM as `.sav` and `.rtc` files.
- `/mnt/data/nes-deck/zx-deck` statically hosts Fuse 1.6.0 at pinned revision
  `bce196fb774835fe65b3e5b821887a4ccf657167`. Its deployed SHA-256 is
  `f541d68fb6c671e3205df37458645863ec43d88c8ccbbaee79987069f9e9436e`.
  It emulates a 48K Spectrum, automatically loads TAP media, maps Player 1 to
  Kempston and Player 2 to Sinclair 2, and renders the medium-border 288x216
  frame at exact 2x scale inside the rounded-screen safe area.
- `/mnt/data/nes-deck/chip8-deck` statically hosts the pinned c-octo core for
  CHIP-8, SCHIP, and XO-CHIP. Its deployed SHA-256 is
  `c4cbdc26f09eb565b3aebaf4068a10c8cdb9274be966b2b6ddcea5cb09d34104`.
  ROM sidecars hold tickrate, palette, quirk, and controller-profile settings.
- The GB/GBC, ZX, and CHIP-8 frontends share exact framebuffer validation,
  nearest-neighbor integer scaling inside a 16-pixel rounded-panel safe area,
  OSS S16 mono audio,
  volume inheritance, frame pacing, and the stable two-controller discovery
  code. Completed frames are built in cacheable RAM and only the active
  rotated rectangle is published to the live scanout, preventing the moving
  black triangle caused by partial framebuffer writes. The RGB565 path writes
  one complete physical row at a time. GB's 160x144 image renders at 3x, ZX's
  288x216 medium-border image at 2x, 64x32 CHIP-8 at 14x, and 128x64
  high-resolution modes at 7x.
- Live Elite and Knight Lore runs detected the two stable controller paths,
  opened 44.1 kHz audio, rendered distinct framebuffer captures, and exited
  cleanly on TERM. Elite's initial accelerated tape load took 16.656 seconds;
  every following 60-frame window held 50 FPS at 1.199 to 1.201 seconds.
  Knight Lore held the same 50 FPS after switching to exact 2x output, with
  52,851 audio frames per 60 callbacks and no dropped samples. Logs and raw
  captures are retained under
  `/mnt/data/nes-deck/log/live-smoke-20260714-zx-spectrum/`.
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
  `2640b2cdfee29d92f70f2fb137e8f25e189892d7e0e69d499a91c9b16018718b`.
  A live launch reached the interactive `root@braiins-deck` prompt. Exiting the
  shell or using the two-second touch hold returns to the menu.
- The terminal launcher accepts only exact `shell`, `lua`, and `lisp` modes.
  Lua starts the pinned interpreter in `/mnt/data/langs/lua`; Lisp exports the
  exact ECL runtime directory, passes `--norc`, and starts in
  `/mnt/data/langs/lisp`. Direct live probes reported Lua 5.5 and ECL 26.5.5
  from those working directories before the menu service was restarted.
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

As of 2026-07-14 the supervised menu is running with one `deck-menu` process
and no unmanaged emulator process. The live generated fifteen-game catalog
matches the fallback, `net1` remains associated with its IPv4 default route,
all 32 saved PSK profiles remain present, and WireGuard is reachable. No wireless
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
The current ZX/menu/timer deployment rollback is
`/mnt/data/nes-deck/backups/20260714-pre-zx-spectrum-d1cd98c/`; it retains the
pre-Spectrum menu payloads and the tested intermediate Spectrum binaries.
The pre-REPL menu, launcher, and terminal files are retained beside their live
counterparts with the suffix `.pre-repl-20260714`.
The menu and launcher immediately preceding the three-card tabbed dashboard are
retained in
`/mnt/data/nes-deck/backups/20260714-pre-three-card-ui/`. The replacement
adopted the live 12-of-20 backlight as persistent 60 percent without changing
the panel level or any network configuration.

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
/mnt/data/nes-deck/langs/lua -e 'print(_VERSION)'
ls -ld /mnt/data/langs/lua /mnt/data/langs/lisp
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
