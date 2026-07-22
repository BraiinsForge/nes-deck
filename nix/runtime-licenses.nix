{
  pkgs,
  pkgsCross,
  staticCross,
  nixpkgsSource,
  nativeCargoDeps,
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

  rust_notices="$licenses/Rust-crates-NOTICES.txt"
  printf '%s\n' 'Rust dependency notices for retrodeck-native' > "$rust_notices"
  append_rust_notice() {
    crate=$1
    file=$2
    printf '\n===== %s/%s =====\n\n' "$crate" "$file" >> "$rust_notices"
    cat "${nativeCargoDeps}/$crate/$file" >> "$rust_notices"
  }
  append_rust_notice bitflags-2.13.1 LICENSE-MIT
  append_rust_notice downcast-rs-1.2.1 LICENSE-MIT
  append_rust_notice linux-raw-sys-0.12.1 LICENSE-MIT
  append_rust_notice memchr-2.8.3 LICENSE-MIT
  append_rust_notice proc-macro2-1.0.107 LICENSE-MIT
  append_rust_notice quick-xml-0.39.4 LICENSE-MIT.md
  append_rust_notice quote-1.0.47 LICENSE-MIT
  append_rust_notice rustix-1.1.4 LICENSE-MIT
  append_rust_notice smallvec-1.15.2 LICENSE-MIT
  append_rust_notice unicode-ident-1.0.24 LICENSE-MIT
  append_rust_notice unicode-ident-1.0.24 LICENSE-UNICODE
  append_rust_notice wayland-backend-0.3.15 LICENSE.txt
  append_rust_notice wayland-client-0.31.14 LICENSE.txt
  append_rust_notice wayland-scanner-0.31.10 LICENSE.txt
  append_rust_notice wayland-sys-0.31.11 LICENSE.txt

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
