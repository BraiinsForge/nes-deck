# ROM intake web server

The native `rom-uploader` service accepts owner-supplied ROMs at the Deck's
active Wi-Fi or WireGuard IPv4 address on port 8080. Deployment writes
`0.0.0.0:8080` to `/mnt/data/nes-deck/uploader/address.conf`, so the listener
accepts connections on every IPv4 interface. Requests must use an IPv4 literal
host on port 8080, and form origins must match that exact address.

The service uses a PBKDF2-HMAC-SHA256 password record, bounded login attempts,
eight-hour same-site sessions, CSRF tokens, strict origin and host checks, and
a 12 MiB request ceiling. It validates raw ROMs or a ZIP containing exactly
one matching ROM. Existing files are never replaced. Successful files go to
`/mnt/data/roms/<system>/`, and the private supplemental catalog is written
atomically to `/mnt/data/nes-deck/uploads/games.tsv` before the dashboard is
restarted.

The authenticated page also edits all semantic dashboard colors as full
`#RRGGBB` values and offers matching previews for the native pixel-cog
settings button. It writes one complete, strictly validated S-expression to
`/mnt/data/nes-deck/state/dashboard-palette.sexp` and restarts the dashboard.
The launcher accepts existing version-2 color overrides, validates version-3
colors and icon together, and keeps built-in defaults available when the
optional override is malformed.

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
