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

The first deployment generates a 27-character password and prints it once.
The digest remains at `/mnt/data/nes-deck/uploader/password.conf`; the clear
password is not stored. To replace it without placing the value in argv or
shell history, run this from a trusted machine:

```sh
read -rsp 'New ROM uploader password: ' password
printf '\n'
printf '%s\n' "$password" |
  ssh root@10.0.0.10 \
    '/mnt/data/nes-deck/uploader/rom-uploader --set-password /mnt/data/nes-deck/uploader/password.conf && /etc/init.d/nes-deck-uploader restart'
unset password
```

Use at least 16 bytes. The service never changes Wi-Fi, WireGuard, routes, or
firewall rules.
