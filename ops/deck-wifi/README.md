# Deck Wi-Fi profile selector

The Deck radio supports only one managed interface, so creating one OpenWrt
`wifi-iface` per saved network is not viable. These scripts retain the existing
single station VIF. The watcher gives the original network a 240-second boot
grace, then requires three consecutive disconnected checks before asking the
selector to investigate.

Canonical IWD `.psk` files live in `/etc/deck-wifi/profiles` with mode `0600`.
The selector decodes IWD filenames, ignores profiles containing
`AutoConnect=false`, and scans without logging SSIDs. It will switch only after
the configured SSID is absent from three separate successful scans. Candidates
must advertise a PSK authentication suite; SAE-only and unclassified BSSes are
skipped. The strongest eligible PSK candidate from the confirming scan wins.
A second association check immediately before commit prevents a scan/reconnect
race, and a live association is never changed.

Every switch is transactional. The previous UCI file is saved immediately
before commit. The candidate gets up to 180 seconds to establish association,
an IPv4 address, and an IPv4 default route. If it fails, the selector atomically
restores that immediate backup, reloads wireless, and waits for the original
configuration to recover before returning control to the watcher.

`/etc/config/wireless`, its pre-switch backups, and the generated supplicant
configuration are forced to mode `0600`. The procd service starts after the
OpenWrt network service and keeps the generated file private when netifd
recreates it.
