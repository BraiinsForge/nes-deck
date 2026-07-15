# Deck userspace WireGuard

This bundle runs WireGuard through TUN, because the Deck's factory 5.10.176
kernel has neither WireGuard nor TUN and its dead target feed cannot safely
supply kernel modules. It contains no private key. The committed `wg0.conf`,
`server-peer.conf`, and all other repository files contain only public
routing/peer information.

## Prerequisite

This bundle includes the ABI-matched `bin/tun.ko` built for this exact Deck
kernel. Install it as `/lib/modules/5.10.176/tun.ko` and install `30-tun` as
`/etc/modules.d/30-tun`. The runner tries `modprobe tun`, verifies
`/sys/class/misc/tun/dev`, and creates `/dev/net/tun` from the sysfs major/minor
when devtmpfs has not created it. It refuses to start if TUN is unavailable.

Do not install a stock-feed kmod whose vermagic differs from
`5.10.176-1-c5bfc45a30e47807303e5abc3fd4a4f1`.

## Deployment layout

Install the files as follows (the data partition is intentionally used for the
3.3 MiB userspace payload):

```text
/mnt/data/nes-deck/wireguard/bin/wireguard-go
/mnt/data/nes-deck/wireguard/bin/wg
/mnt/data/nes-deck/wireguard/deck-wireguard-run
/lib/modules/5.10.176/tun.ko
/etc/modules.d/30-tun
/etc/wireguard/wg0.conf
/etc/wireguard/wg0.key       # generated on Deck later; never commit it
/etc/wireguard/wg0.address   # per-Deck 10.0.0.x/32 address
/etc/init.d/deck-wireguard
```

Use modes `0755` for binaries/scripts, `0600` for all files under
`/etc/wireguard`, and `0755` for the init script. At deployment time, generate
the Deck private key locally on the Deck into `/etc/wireguard/wg0.key`, derive
its public key, choose an unused address, write that address as a `/32` to
`wg0.address`, and only then add the public key and `/32` as a server peer. On
the audited Deck those steps have been completed and verified across reboot;
the private key remains only on that Deck and is intentionally absent here.

The procd service starts at priority 96, after `/mnt/data` is mounted at 90.
It supervises `wireguard-go --foreground`, configures the address in
`wg0.address`, and
routes only `10.0.0.0/24` through the tunnel. It does not need to wait for the
slow Realtek Wi-Fi association: the endpoint is an IP address and the 25-second
keepalive will establish a handshake after connectivity appears.

After deployment, enable and verify with:

```sh
/etc/init.d/deck-wireguard enable
/etc/init.d/deck-wireguard start
/mnt/data/nes-deck/wireguard/bin/wg show wg0
ip address show dev wg0
ip route show 10.0.0.0/24
logread -e deck-wireguard
```

## Binary provenance

- `wireguard-go` version `0.0.20250522`, upstream commit
  `f333402bd9cbe0f3eeb02507bd14e23d7d639280`, built with Go 1.25.4,
  `CGO_ENABLED=0 GOOS=linux GOARCH=arm GOARM=7`.
- `wg` from wireguard-tools `1.0.20260223`, upstream commit
  `49ce333da02056ae7b22ee2aeb6afe8aaed79b19`, built with Zig 0.12.0 for
  `arm-linux-musleabihf -mcpu=cortex_a7` and statically linked against musl.

Both upstream repositories are `https://git.zx2c4.com/`. `build-userspace.sh`
pins the peeled commits rather than mutable branches. `SHA256SUMS` records the
exact payloads built on `root@10.0.0.1`.

`tun.ko` comes from Braiins `linux-stm` commit
`2aca87d7aa4707aa42bbbfd2a6868df15d4df916` with the running kernel config
(SHA-256 `9974ac5d8188db909341cf89b78cf272e261f24306e5ef566bc26fe71d08a126`).
It has vermagic `5.10.176 SMP preempt mod_unload modversions ARMv7 p2v8`.
All 194 imported symbol CRC values were matched against the shipped zImage,
and it was test-loaded normally. Never use `insmod -f` or force-modversion.

The checked-in binary blobs are intentional so the old, storage-constrained
Deck can be recovered without rebuilding a toolchain. If repository size is a
concern, put these two immutable files in a release/artifact store instead and
keep this directory's build script plus checksums; do not replace them with an
unpinned download at install time.
