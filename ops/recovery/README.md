# Presentation recovery

These helpers switch a Wayland-capable Deck between the BMC compositor and
Retro Deck's direct-framebuffer mode without downloading or replacing the
emulators.

On a prepared Deck:

```sh
/mnt/data/nes-deck/recovery/use-framebuffer
/mnt/data/nes-deck/recovery/use-wayland
/mnt/data/nes-deck/recovery/retro-deck-presentation status
```

The framebuffer command writes
`/mnt/data/nes-deck/state/force-framebuffer`. The
`retro-deck-presentation` boot service checks that marker after Nix activation
and reapplies framebuffer ownership, so the fallback survives a reboot. The
Wayland command removes the marker and restores compositor ownership.

Both transitions verify that the selected service and `deck-menu` started. If
framebuffer startup fails, the command automatically restores Wayland. If
Wayland startup fails, it automatically restores the framebuffer.
