# BMC Wayland canary

Use the current `feature/retro-deck-widget` BMC line for Retro Deck. The older
`feature/retro-dashboard-wayland` line is retained for archaeology and rollback
comparison, not as the release candidate. The current line contains the older
buffer-lifetime fixes plus native application supervision, centralized audio
and input, prepared swipe neighbors, and the later lifecycle fixes.

This procedure installs Nix-managed BMC packages into one new profile
generation. It is not a BOS sysupgrade. It must not edit, reload, select, or
disconnect Wi-Fi.

## Prepare immutable sources

Start from a clean Retro Deck commit. Read its pinned BMC revision, create a
detached BMC worktree outside either repository, and apply the tracked display
filter patch:

```sh
cd /path/to/retrodeck
BMC_REV=$(jq -r '.nodes["bmc-main"].locked.rev' flake.lock)
RETRO_REV=$(git rev-parse HEAD)
STATE=$HOME/.local/state/retro-deck/wayland-canary-$RETRO_REV
install -d -m 0700 "$STATE"
git -C /path/to/bmc-main worktree add --detach "$STATE/bmc-main" "$BMC_REV"
./ops/bmc/apply-local-patches.sh "$STATE/bmc-main"
```

The patch deliberately remains in Retro Deck instead of BMC history. Verify
that the BMC worktree contains only that patch:

```sh
git -C "$STATE/bmc-main" diff --check
git -C "$STATE/bmc-main" status --short
```

## Build and test

Run the complete Retro Deck checks, build all static ARM payloads, and build
the three canary packages. Build before `nix flake check --no-build` so a fresh
Nix store can materialize Cargo metadata used by the external widget builder:

```sh
./tests/run-host-tests.sh
./tests/verify-arm-builds.sh
nix build --no-link --print-out-paths \
  "path:$STATE/bmc-main#legacyPackages.x86_64-linux.deck-packages.core.pkg" \
  "path:$STATE/bmc-main#legacyPackages.x86_64-linux.deck-packages.widget-clock.pkg" \
  "git+file://$PWD?rev=$RETRO_REV#legacyPackages.x86_64-linux.deck-packages.retro-deck.pkg"
nix flake check --no-build
```

Run the BMC compositor tests in the staged worktree. The core package build
above also compiles the external display patch for ARM:

```sh
cargo test -p bmc-openwrt --manifest-path "$STATE/bmc-main/Cargo.toml"
```

## Deploy one canary

Do not use a conference or otherwise irreplaceable Deck as the first target.
Record the current generation before any mutation:

```sh
DECK=<deck-host>
ssh root@$DECK 'readlink -f /nix/var/nix/gcroots/profiles/bmc/current'
ssh root@$DECK 'ls -ld /nix/var/nix/gcroots/profiles/bmc/[0-9]*-link'
```

The deployment tool accepts package attributes from different flakes. Probe
the Deck and rebuild the exact set without changing it first:

```sh
nix run .#deck -- deploy --device "$DECK" --dry-run --packages \
  "path:$STATE/bmc-main#deck-packages.core" \
  "path:$STATE/bmc-main#deck-packages.widget-clock" \
  "git+file://$PWD?rev=$RETRO_REV#deck-packages.retro-deck"
```

Remove `--dry-run` for the real canary and accept the compositor restart. The
tool copies verified Nix closures and activates one new profile generation. It
leaves the previous generation available for rollback.

If the recorded old generation was `N`, roll back with:

```sh
ssh root@$DECK \
  '/nix/var/nix/gcroots/profiles/bmc/current/bin/bmc-nix-cli activate --generation N'
```

Do not garbage-collect old generations until repeated swipes, emulators,
audio, input, return-to-dashboard behavior, and network reachability have all
passed physical acceptance. The current checklist is in `DECK_NOTES.md`.
