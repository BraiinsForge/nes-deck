# Vendored fbterm source

The source in `fbterm/` is vendored from Braiins Forge's fbterm fork at
commit `14d0e1e3f03f75b3f6fce9c73e8885cc2259f0ae`. It was referenced through
`git@github.com:BraiinsForge/fbterm-deck-nix.git` at wrapper commit
`db52666c0b51d817d24e7721e76d48272fa7293f`.

Local changes validate the Deck's real 600x1280 RGB565 framebuffer, restrict
rendering to the rotated 1280x480 visible region with a 16-pixel rounded-corner
safe area, use reported RGB565 channel ordering, require stable monospaced font
advances, and harden glyph clipping.

fbterm is licensed under GPL-2. The complete license is in
`fbterm/COPYING`. The wrapper's old installation guides were intentionally
removed: they described a separate precompiled release, obsolete fonts, and a
manual service takeover that does not match Retro Deck's integrated build.

The build bundles unmodified DejaVu Sans Mono 2.37 from nixpkgs. Its complete
Bitstream Vera/DejaVu license notice is retained in `fonts/DejaVu-LICENSE` and
installed beside fbterm's GPL-2 license.

The same build bundles the static `loadkeys` executable and console maps from
the kbd 2.7.1 package in the locked nixpkgs revision. The terminal uses the
upstream `i386/qwerty/us.map` and `i386/qwertz/cz-qwertz.map` definitions plus
the US map's explicit include files. The complete kbd GPL-2 license is
installed as `kbd-COPYING` beside the terminal licenses.
