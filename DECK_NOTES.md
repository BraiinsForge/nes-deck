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
- InfoNES renders the 256x240 frame at integer 2x scale. Its 512x480 output is
  centered at logical x=384..895 and fills the active y=0..479 panel. Earlier
  offsets shifted it 25 pixels left, left a 40-pixel gap above it, and clipped
  the final 20 NES scanlines.

## Touchscreen

- `/dev/input/event0` is `Goodix Capacitive TouchScreen`. It reports the
  intended landscape coordinates directly: ABS_X 0..1279, ABS_Y 0..479, and
  BTN_TOUCH, plus multitouch slots. No rotation or calibration is required.
- The S99 menu exclusively grabs the Goodix event device while it is active.
  It releases the framebuffer before starting InfoNES but retains touch input
  so a continuous two-second hold anywhere on the touchscreen can terminate
  the game and return to the menu. Touch is not otherwise used during gameplay.
- The menu maps its 1280x480 logical canvas onto the portrait framebuffer as
  `column = y`, `row = 1279 - x`. Its host test verifies every logical pixel
  maps in bounds and exactly once.

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
- The menu's top-right sound button persists exact `on\n` or `off\n` state in
  `/mnt/data/nes-deck/state/menu-sound.state`. ON launches the emulator with
  inherited `INFONES_VOLUME_PERCENT=42`; OFF launches it with 0. The selection
  affects the next game and survives service restarts and reboot. Toggling
  from OFF to ON also plays a short two-note confirmation through `/dev/dsp`,
  so the setting can be checked without starting a game.
- The deployed S16 build has SHA-256
  `08c4b58a491552e41bda095c8e83081b115d1110cc5e032135ee8b95ac8cd0fa`.
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
  them for rollback.
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
  `/mnt/data/nes-deck/menu/deck-menu-launcher`, sends output to logd, and uses
  bounded crash respawning. The launcher validates the source catalog with ECL
  and then execs the native menu. The previous direct-Mario init script is
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
  `14d5cd74c88abc4b502e3981a0b2b964aeee440d7222b3c50829b6c0dd19ebf2`.
  It validates the manifest and iNES headers before opening the framebuffer,
  supervises one emulator child, and restores tty state after the child exits.
  This build includes the full-screen two-second return hold, the two-line
  `NEXT GAME / SOUND ON|OFF` button, and the sound-on confirmation chime.
- `games.sexp` is the editable schema-checked source. At each service start,
  the ECL compiler atomically generates
  `/mnt/data/nes-deck/state/games.tsv`; the checked-in TSV is a known-good
  fallback. The actual Deck ECL output was verified byte-for-byte against that
  fallback for all five entries. The final deployed catalog/fallback SHA-256
  was `190e128f5d0f7ca06c0df8fca61f4c171331d98b54986be773f1f71a940fef6d`.
- Four pinned, freely licensed mapper-0 homebrew releases are installed:
  Falling, Thwaite, Concentration Room, and robotfindskitten. Provenance,
  license texts, and ROM hashes are recorded in [FOSS_GAMES.md](FOSS_GAMES.md).
  The fifth entry, `mario.nes`, is user-supplied and is not redistributed.

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
- The first deployed selector had no boot grace and began active scans while
  the Realtek driver was still in its ~155-second startup recovery. After both
  the workstation and Deck were rebooted, the Deck was reachable on both LAN
  and WireGuard; the earlier conclusion that Ethernet recovery was required
  was incorrect.
- The installed scripts now match the hardened repository copies: a 240-second
  grace, three confirming scans, PSK-only candidate selection, and automatic
  rollback. At the user's direction the `deck-wifi` watcher is currently
  stopped and disabled so it cannot disturb the working link. All 32 imported
  root-only profiles remain stored, but automatic profile failover is inactive.

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

The last remote session ended while restoring the supervised menu after a
direct Mario audio diagnostic, and the Deck was then taken off-site. Do not
assume the final restore command completed. On the next connection, first
ensure there is no unmanaged `infones` process and that procd owns exactly one
`deck-menu`; if necessary, stop the exact diagnostic emulator process and
start `/etc/init.d/nes-deck`. Do not reboot or alter Wi-Fi/WireGuard merely to
perform this check. The Deck retains its Wi-Fi profiles and WireGuard key, so
the user's other laptop should be able to reach it over the existing tunnel
when both are online; no credential migration is required. The two newest
interaction changes still need physical
confirmation: tap SOUND OFF and then SOUND ON to hear the chime, then launch a
game and hold anywhere continuously for two seconds to return. Relevant log
messages are `return hold started`, `return hold cancelled`, and `return hold
complete`.

```sh
# Stock UI must stay disabled
/etc/init.d/bmc enabled; echo "$?"   # expected nonzero
pgrep -x bmc                         # expected no output

# Touch-menu service
/etc/init.d/nes-deck status
pidof deck-menu
pidof infones
logread -e nes-deck-menu
hexdump -C /mnt/data/nes-deck/state/menu-sound.state

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
