# Deck Wi-Fi profile selector

The Deck radio supports only one managed interface, so creating one OpenWrt
`wifi-iface` per saved network is not viable. These scripts retain the existing
single station VIF. The watcher gives the configured network a 90-second boot
grace, then requires two consecutive failures of complete network health before
asking the selector to investigate. Complete health means association, an IPv4
address on `wlan0`, and an IPv4 default route through `wlan0`.

Canonical IWD `.psk` files live in `/etc/deck-wifi/profiles` with mode `0600`.
The selector decodes IWD filenames, ignores profiles containing
`AutoConnect=false`, and scans without logging SSIDs. Candidates must advertise
a PSK authentication suite; SAE-only and unclassified BSSes are skipped. After
two successful scans, every visible known candidate is tried once in signal
order, with alternatives ahead of the currently configured SSID. Both IWD
`Passphrase` and 64-digit `PreSharedKey` profiles are supported. A complete
network-health check immediately before every commit prevents a scan/reconnect
race, and a healthy connection is never changed.

Every selection run is transactional. The previous UCI file is saved once
immediately before the first candidate. Each candidate gets up to 60 seconds to
establish complete network health. If every candidate fails, the selector
atomically restores that immediate backup, allows a bounded 20-second recovery
grace, and returns control to the watcher even when the unavailable original
network does not recover. It never waits forever after rollback.

The watcher and selector atomically maintain the root-only runtime state file
`/var/run/deck-wifi/status`. It contains short credential-free states such as
`BOOT GRACE 90 SECONDS`, `SCANNING KNOWN WIFI`, `TRYING KNOWN WIFI 1 OF 3`,
and `NO KNOWN WIFI CONNECTED`. The dashboard displays this state beside the
active SSID and the `wlan0` and `wg0` IPv4 addresses.

`/etc/config/wireless`, its pre-switch backups, and the generated supplicant
configuration are forced to mode `0600`. The procd service starts after the
OpenWrt network service and keeps the generated file private when netifd
recreates it.

For a fresh Deck, `ops/provision-deck.sh` copies regular `.psk` files from the
development machine's `/var/lib/iwd` directory by default. It never imports
`.open` or `.8021x` profiles. Before and after installation it compares the
live UCI file hash, `wlan0` address, and full default route, and refuses to
continue to application deployment if any of them changed.

`deck-wifi-profile-add` is the write-only companion used by the Retro Deck
menu. It reads exactly two lines from stdin (SSID and PSK passphrase), validates
printable-ASCII PSK limits, and atomically writes a mode-0600 canonical IWD
profile. If the SSID already exists, all other filename spellings are removed
only after the replacement is committed. It intentionally performs no Wi-Fi
operation, so saving cannot interrupt the current association. Its host test
can be run with `tests/deck_wifi_profile_add_test.sh`.
