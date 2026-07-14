# WireGuard ROM intake

The native `rom-uploader` service accepts owner-supplied ROMs at
`http://10.0.0.10:8080`. Its listener is fixed to the Deck's `10.0.0.10`
address and uses `SO_BINDTODEVICE` for `wg0`; startup fails rather than falling
back to Wi-Fi or an all-address listener.

The service uses a PBKDF2-HMAC-SHA256 password record, bounded login attempts,
eight-hour same-site sessions, CSRF tokens, strict origin and host checks, and
a 12 MiB request ceiling. It validates raw ROMs or a ZIP containing exactly
one matching ROM. Existing files are never replaced. Successful files go to
`/mnt/data/roms/<system>/`, and the private supplemental catalog is written
atomically to `/mnt/data/nes-deck/uploads/games.tsv` before the dashboard is
restarted.

`ops/configure-deck.sh` asks for the uploader password during setup and stores
it in the local, Git-ignored `deck.conf` with mode `0600`. Each deployment
derives a fresh password record and installs only that record at
`/mnt/data/nes-deck/uploader/password.conf`; the clear password is not retained
on the Deck. Change the local configuration and deploy again to rotate it.

To replace it directly without placing the value in argv or shell history,
run this from a trusted machine:

```sh
read -rsp 'New ROM uploader password: ' password
printf '\n'
printf '%s\n' "$password" |
  ssh root@10.0.0.10 \
    '/mnt/data/nes-deck/uploader/rom-uploader --set-password /mnt/data/nes-deck/uploader/password.conf && /etc/init.d/nes-deck-uploader restart'
unset password
```

Eight bytes are accepted for a Deck that remains in a trusted location. Use at
least 16 bytes for a Deck that may leave one. The service never changes Wi-Fi,
WireGuard, routes, or firewall rules.
