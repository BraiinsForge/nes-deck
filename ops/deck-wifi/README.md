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
a PSK authentication suite; SAE-only and unclassified BSSes are skipped. The
selector merges three independent OpenWrt `iwinfo` scans so one missed beacon
cannot erase a saved network seen by another scan. A raw `iw` scan remains as a
compatibility fallback, and three bounded retries per round absorb transient
driver-busy failures. Visible known candidates are tried in signal order, with
alternatives ahead of the currently configured SSID. The complete candidate
set receives a second pass before rollback. Every remaining usable saved PSK
is appended as a directed-association fallback, because a driver or busy access
point can omit a connectable SSID from every scan. Saved profiles are still
tried when both scan providers fail completely. Both IWD `Passphrase` and
64-digit `PreSharedKey` profiles are supported. A complete network-health check
immediately before every commit prevents a scan/reconnect race, and a healthy
connection is never changed.

Every selection run is transactional. The previous UCI file is saved once
immediately before the first candidate. A station that never associates is
released after 30 seconds; an associated candidate gets up to 60 seconds per
pass to establish complete network health. An associated station that is still
missing IPv4 or its default route after 20 seconds receives one bounded netifd
renewal. If both candidate passes fail, the selector atomically restores that
immediate backup, allows a bounded 20-second recovery grace, and returns control
to the watcher even when the unavailable original network does not recover. It
never waits forever after rollback. Every recovered network path restarts the
userspace WireGuard service so its UDP socket and routes follow the new uplink.

The watcher and selector atomically maintain the root-only runtime state file
`/var/run/deck-wifi/status`. It contains short credential-free states such as
`BOOT GRACE 90 SECONDS`, `WIFI SCAN 2 OF 3`,
`WIFI PASS 1 OF 2 NETWORK 1 OF 3`, and `NO KNOWN WIFI CONNECTED`. Each
credential-free transition is also sent to logd for post-outage diagnosis.
Failed health checks record only association, IPv4, and default-route booleans,
never SSIDs or credentials. The dashboard displays current state beside the
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
