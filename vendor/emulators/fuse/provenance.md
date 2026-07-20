# Fuse provenance

- Upstream: https://github.com/libretro/fuse-libretro
- Revision: `bce196fb774835fe65b3e5b821887a4ccf657167`
- Primary license: GPL-3.0-only, retained verbatim in `LICENSE.txt`
- Additional notices: upstream `libspectrum/COPYING` and `bzip2/LICENSE`
- Source input: `fuse-src` in the repository flake
- Integration: pinned libretro core statically linked into the Rust host

Local patches are applied in the order listed by `patches/series`. The pinned
revision currently needs no source patches.
