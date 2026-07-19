{
  pkgs,
  pkgsCross,
  staticCross,
  nixpkgsSource,
}:

let
  wayland = staticCross.wayland;
  libpng = staticCross.libpng;
  zlib = staticCross.zlib;
  libffi = staticCross.libffi;
  glibc = pkgsCross.glibc;
  go = pkgs.go;
in
pkgs.runCommand "retro-deck-runtime-licenses" {
  allowedReferences = [ ];
  nativeBuildInputs = [
    pkgs.gnutar
    pkgs.gzip
    pkgs.xz
  ];

  meta.description = "License notices for shared Retro Deck dependencies";
} ''
  licenses=$out/share/licenses/runtime
  mkdir -p "$licenses"

  tar -xOf ${wayland.src} wayland-${wayland.version}/COPYING \
    > "$licenses/Wayland-COPYING"
  install -m444 ${../protocol/wlr-layer-shell-LICENSE} \
    "$licenses/wlr-layer-shell-LICENSE"
  tar -xOf ${libpng.src} libpng-${libpng.version}/LICENSE \
    > "$licenses/libpng-LICENSE"
  tar -xOf ${zlib.src} zlib-${zlib.version}/LICENSE \
    > "$licenses/zlib-LICENSE"
  tar -xOf ${libffi.src} libffi-${libffi.version}/LICENSE \
    > "$licenses/libffi-LICENSE"
  tar -xOf ${glibc.src} glibc-${glibc.version}/COPYING \
    > "$licenses/glibc-COPYING"
  tar -xOf ${glibc.src} glibc-${glibc.version}/COPYING.LIB \
    > "$licenses/glibc-COPYING.LIB"
  tar -xOf ${go.src} go/LICENSE > "$licenses/Go-LICENSE"
  install -m444 ${nixpkgsSource}/COPYING "$licenses/Nixpkgs-COPYING"
  install -m444 ${../assets/settings-cog/UPSTREAM.txt} \
    "$licenses/knekko-CC0-NOTICE.txt"
  install -m444 ${../deploy/menu/ASSETS.md} \
    "$licenses/menu-assets-provenance.md"
  install -m444 ${../chiptunes/README.md} \
    "$licenses/chiptunes-CC0-provenance.md"
''
