#!/bin/sh

# Rebuild the two static ARMv7 binaries with Go 1.25.4 (X:nodwarf5) and
# Zig 0.12.0.

set -eu

WG_GO_COMMIT=f333402bd9cbe0f3eeb02507bd14e23d7d639280
WG_GO_VERSION=0.0.20250522
WG_TOOLS_COMMIT=49ce333da02056ae7b22ee2aeb6afe8aaed79b19
WG_TOOLS_VERSION=1.0.20260223
OUT_DIR="${1:-$PWD/out}"
GO="${GO:-go}"
ZIG="${ZIG:-zig}"

work="$(mktemp -d "${TMPDIR:-/tmp}/deck-wireguard-build.XXXXXX")"
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

git clone --quiet https://git.zx2c4.com/wireguard-go "$work/wireguard-go"
git -C "$work/wireguard-go" checkout --quiet "$WG_GO_COMMIT"
[ "$(git -C "$work/wireguard-go" rev-parse HEAD)" = "$WG_GO_COMMIT" ]
(
	cd "$work/wireguard-go"
	printf 'package main\n\nconst Version = "%s"\n' "$WG_GO_VERSION" > version.go
	env CGO_ENABLED=0 GOOS=linux GOARCH=arm GOARM=7 GOTOOLCHAIN=local \
		"$GO" build -mod=readonly -trimpath -buildvcs=false \
		-ldflags='-s -w -buildid=' -o "$OUT_DIR/wireguard-go" .
)

git clone --quiet https://git.zx2c4.com/wireguard-tools "$work/wireguard-tools"
git -C "$work/wireguard-tools" checkout --quiet "$WG_TOOLS_COMMIT"
[ "$(git -C "$work/wireguard-tools" rev-parse HEAD)" = "$WG_TOOLS_COMMIT" ]
(
	cd "$work/wireguard-tools/src"
	make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf 1)" \
		CC="$ZIG cc -target arm-linux-musleabihf -mcpu=cortex_a7" \
		CFLAGS="-Os -Iuapi/linux -std=gnu99 -D_GNU_SOURCE -Wall -Wextra -DWIREGUARD_TOOLS_VERSION=\\\"$WG_TOOLS_VERSION\\\" -DRUNSTATEDIR=\\\"/var/run\\\"" \
		LDFLAGS='-static -s' WITH_BASHCOMPLETION=no WITH_WGQUICK=no \
		WITH_SYSTEMDUNITS=no wg
	install -m 0755 wg "$OUT_DIR/wg"
)

chmod 0755 "$OUT_DIR/wireguard-go" "$OUT_DIR/wg"
sha256sum "$OUT_DIR/wireguard-go" "$OUT_DIR/wg"
