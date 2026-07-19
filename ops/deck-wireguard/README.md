# Deck userspace WireGuard

This bundle runs WireGuard through TUN, because the Deck's factory 5.10.176
kernel has neither WireGuard nor TUN and its dead target feed cannot safely
supply kernel modules. It contains no VPN endpoint, peer key, address, route,
or private key. Those belong in the operator's private configuration outside
the repository.

## Prerequisite

This bundle includes the ABI-matched `bin/tun.ko` built for this exact Deck
kernel. Install it as `/lib/modules/5.10.176/tun.ko` and install `30-tun` as
`/etc/modules.d/30-tun`. The runner tries `modprobe tun`, verifies
`/sys/class/misc/tun/dev`, and creates `/dev/net/tun` from the sysfs major/minor
when devtmpfs has not created it. It refuses to start if TUN is unavailable.

Do not install a stock-feed kmod whose vermagic differs from
`5.10.176-1-c5bfc45a30e47807303e5abc3fd4a4f1`.

`ops/provision-deck.sh` reads the client `setconf` input and peer-registration
command from the operator's config directory, outside this checkout. The
client file must not contain a private key. Its `AllowedIPs` value must match
the routed prefix in the selected per-Deck configuration. Start from
`wg0.conf.example`, replace the placeholder server identity and endpoint, and
store the resulting file outside the checkout with mode `0600`.

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
/etc/wireguard/wg0.address   # operator-configured per-Deck IPv4 /32
/etc/wireguard/wg0.route     # operator-configured routed IPv4 prefix
/etc/init.d/deck-wireguard
```

Use modes `0755` for binaries/scripts, `0600` for all files under
`/etc/wireguard`, and `0755` for the init script.
`ops/provision-deck.sh` performs the per-Deck steps in a guarded order: it
generates the private key on the Deck, refuses to replace an existing address
or private key, calls an explicit external peer-registration command, and only
then starts the client tunnel. The repository does not know how or where the
operator's VPN server is managed. The private key never leaves the Deck; only
its derived public key is passed to that external command. The network-only
path is idempotent and is suitable for verifying an existing installation.

The procd service starts at priority 96, after `/mnt/data` is mounted at 90.
It supervises `wireguard-go --foreground`, configures the address in
`wg0.address`, and installs the prefix from `wg0.route`. It does not assume a
particular private subnet or server address.

After deployment, enable and verify with:

```sh
/etc/init.d/deck-wireguard enable
/etc/init.d/deck-wireguard start
/mnt/data/nes-deck/wireguard/bin/wg show wg0
ip address show dev wg0
ip route show "$(cat /etc/wireguard/wg0.route)"
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
exact payloads produced by the pinned cross-build.

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
