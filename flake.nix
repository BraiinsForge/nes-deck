{
  description = "Retro Deck emulators and launcher for Braiins Forge Deck";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    bmc-main.url = "git+ssh://git@gitlab.ii.zone/bos/bmc-main.git?rev=bf4004e3c2fdcb4224c060ab7657ba1338e098cb";
    fceumm-src = {
      url = "github:libretro/libretro-fceumm/3a84a6fd0ba20dd4877c06b1d58741172148395f";
      flake = false;
    };
    gambatte-src = {
      url = "github:libretro/gambatte-libretro/dfc165599f3f1068c40a0b7ad6fe5f161283d483";
      flake = false;
    };
    fuse-src = {
      url = "github:libretro/fuse-libretro/bce196fb774835fe65b3e5b821887a4ccf657167";
      flake = false;
    };
    lua-src = {
      url = "https://www.lua.org/ftp/lua-5.5.0.tar.gz";
      flake = false;
    };
  };

  outputs =
    { self, nixpkgs, bmc-main, fceumm-src, gambatte-src, fuse-src, lua-src }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      pkgsCross = pkgs.pkgsCross.armv7l-hf-multiplatform;
      staticCross = pkgs.pkgsCross.armv7l-hf-multiplatform.pkgsStatic;
      # Nixpkgs vendors every Git source named by the shared lock, even when a
      # package does not enable bmc-native. All BMC crates use this one revision,
      # so one source hash covers the complete Git workspace.
      cargoLock = {
        lockFile = ./Cargo.lock;
        outputHashes = {
          "bmc-render-0.1.0" = "sha256-9ouWZND/DOod9ZTHivlXCq2n3Az90Tp0B3yZ6pVui+E=";
          "smithay-0.7.0" = "sha256-7oa5N61giQTl9cWSrY+Ap8rlHP4zeNJyN83w8swTqSo=";
        };
      };
      nativeDashboardWidget = bmc-main.bmc.${system}.lib.mkExternalNativeWidgetPackage {
        name = "retro-deck";
        src = ./.;
        cratePath = "crates/retro-deck-dashboard";
        packageName = "retro-deck-dashboard";
        binName = "retro-deck";
        features = [ "bmc-native" ];
        noDefaultFeatures = true;
        manifest = ./deploy/widget/manifest.json;
        application = {
          name = "retro-deck";
          packageName = "retro-deck-dashboard";
          binName = "retro-deck-launcher";
          features = [ "application-launcher" ];
          noDefaultFeatures = true;
          manifest = ./deploy/application/manifest.json;
        };
      };

      waylandNativeInputs = [ pkgs.wayland-scanner ];
      waylandStaticInputs = [ staticCross.wayland staticCross.libffi ];
      # Keep each local build input narrow. Referencing ./src as an include
      # directory would make every source edit invalidate every native runtime.
      sourceTree = files: pkgs.lib.fileset.toSource {
        root = ./src;
        fileset = pkgs.lib.fileset.unions files;
      };
      menuSources = sourceTree [
        ./src/deck_menu.cpp
        ./src/deck_wayland.cpp
        ./src/deck_wayland.h
        ./src/menu_catalog.cpp
        ./src/menu_catalog.h
        ./src/menu_credits.cpp
        ./src/menu_credits.h
        ./src/menu_io.cpp
        ./src/menu_io.h
        ./src/menu_network.cpp
        ./src/menu_network.h
        ./src/menu_sound.cpp
        ./src/menu_sound.h
        ./src/menu_state.cpp
        ./src/menu_state.h
        ./src/menu_text.cpp
        ./src/menu_text.h
        ./src/menu_ui.cpp
        ./src/menu_ui.h
      ];
      waylandProtocolBuild = ''
        wayland-scanner client-header \
          ${./protocol/deck-widget-v1.xml} \
          deck-widget-v1-client-protocol.h
        wayland-scanner private-code \
          ${./protocol/deck-widget-v1.xml} \
          deck-widget-v1-protocol.c
        wayland-scanner client-header \
          ${./protocol/wlr-layer-shell-unstable-v1.xml} \
          wlr-layer-shell-unstable-v1-client-protocol.h
        wayland-scanner private-code \
          ${./protocol/wlr-layer-shell-unstable-v1.xml} \
          wlr-layer-shell-unstable-v1-protocol.c
        $CC -std=c99 -Os -Wall -Wextra -Werror \
          -c deck-widget-v1-protocol.c -o deck-widget-v1-protocol.o
        $CC -std=c99 -Os -Wall -Wextra -Werror \
          -c wlr-layer-shell-unstable-v1-protocol.c \
          -o wlr-layer-shell-unstable-v1-protocol.o
      '';
      runtimeLicenses = import ./nix/runtime-licenses.nix {
        inherit pkgs pkgsCross staticCross;
        nixpkgsSource = nixpkgs.outPath;
      };
      rustWorkspaceSources = extraFiles: pkgs.lib.fileset.toSource {
        root = ./.;
        fileset = pkgs.lib.fileset.unions (
          [ ./Cargo.lock ./Cargo.toml ./crates ] ++ extraFiles
        );
      };
      uploaderSources = rustWorkspaceSources [
        ./deploy/menu/games.tsv
        ./deploy/menu/palette.tsv
      ];
      timerRustSources = rustWorkspaceSources [
        ./protocol/deck-widget-v1.xml
      ];
      chiptuneRustSources = rustWorkspaceSources [
        ./protocol/deck-widget-v1.xml
      ];
      chip8RustSources = rustWorkspaceSources [
        ./protocol/deck-widget-v1.xml
        ./vendor/emulators/c-octo/LICENSE.txt
        ./vendor/emulators/c-octo/upstream/octo_emulator.h
      ];
      libretroRustSources = rustWorkspaceSources [
        ./protocol/deck-widget-v1.xml
      ];
      nativeStaticLibraries = [
        "-lm"
        "-lutil"
        "-lrt"
        "-lpthread"
        "-ldl"
        "-lc"
        "-lgcc_eh"
        "-lgcc"
      ];
      staticArchiveLinkFlags = archive: nativeLibraries:
        pkgs.lib.concatMapStringsSep " "
          (argument: "-C link-arg=${argument}")
          (
            [ "-Wl,--start-group" "-Wl,-Bstatic" archive ]
            ++ nativeLibraries
            ++ nativeStaticLibraries
            ++ [ "-Wl,--end-group" ]
          );
      mkLibretroHost =
        {
          pname,
          version,
          coreBuild,
          coreArchive,
          nativeLibraries ? [ ],
          extraNativeBuildInputs ? [ ],
          extraBuildInputs ? [ ],
          installLicenses,
          description,
          homepage,
          license,
        }:
        pkgsCross.rustPlatform.buildRustPackage {
          inherit pname version;

          src = libretroRustSources;
          inherit cargoLock;
          cargoBuildFlags = [
            "-p"
            "retro-deck-emulator"
            "--bin"
            "libretro-deck"
            "--no-default-features"
            "--features"
            "libretro-linked"
          ];
          doCheck = false;

          env.RUSTFLAGS = "-C target-feature=+crt-static -C link-arg=-static";
          nativeBuildInputs = [ pkgs.gnumake pkgs.nukeReferences ]
            ++ extraNativeBuildInputs;
          buildInputs = [ pkgsCross.glibc.static ] ++ extraBuildInputs;
          allowedReferences = [ ];

          preBuild = coreBuild + ''
            export RUSTFLAGS="$RUSTFLAGS ${staticArchiveLinkFlags coreArchive nativeLibraries}"
          '';

          postInstall = ''
            mkdir -p $out/bin $out/share/licenses/${pname}
            mv $out/bin/libretro-deck $out/bin/${pname}
          '' + installLicenses;

          postFixup = ''
            ${pkgsCross.stdenv.cc.bintools.bintools}/bin/${pkgsCross.stdenv.cc.targetPrefix}strip \
              --strip-all $out/bin/${pname}
            nuke-refs $out/bin/${pname}
          '';

          meta = {
            inherit description homepage license;
            platforms = [ "armv7l-linux" ];
          };
        };
      uploaderPackage = {
        pname = "rom-uploader";
        version = "0.1.0";
        src = uploaderSources;
        inherit cargoLock;
        cargoBuildFlags = [ "-p" "retro-deck-uploader" ];
        doCheck = false;

        postInstall = ''
          mv $out/bin/retro-deck-uploader $out/bin/rom-uploader
        '';
      };

    in
    {
      packages.${system} = {
        runtime-licenses = runtimeLicenses;
        retro-deck-widget = nativeDashboardWidget;

        nes-deck = mkLibretroHost {
          pname = "nes-deck";
          version = "0.2.0";
          coreBuild = ''
            core=$TMPDIR/fceumm-core
            cp -R ${fceumm-src} "$core"
            chmod -R u+w "$core"
            patch_root=${./vendor/emulators/fceumm/patches}
            while IFS= read -r local_patch || [ -n "$local_patch" ]; do
              case "$local_patch" in
                ""|\#*) continue ;;
              esac
              patch -d "$core" -p1 < "$patch_root/$local_patch"
            done < "$patch_root/series"
            # A standalone static frontend needs the core's vendored libretro
            # utility implementations instead of symbols from RetroArch.
            substituteInPlace "$core/Makefile.common" \
              --replace-fail \
                'ifneq ($(STATIC_LINKING), 1)' \
                'ifeq ($(STATIC_LINKING), 1)'
            make -C "$core" -j$NIX_BUILD_CORES \
              platform=rpi2 \
              STATIC_LINKING=1 \
              TARGET=fceumm_libretro.a \
              EXTERNAL_ZLIB=1 \
              CC=$CC \
              AR=${pkgsCross.stdenv.cc.targetPrefix}ar
          '';
          coreArchive = "$core/fceumm_libretro.a";
          nativeLibraries = [ "-lstdc++" "-lz" ];
          extraNativeBuildInputs = [ pkgs.gnupatch ];
          extraBuildInputs = [ staticCross.zlib ];
          installLicenses = ''
            install -m644 ${fceumm-src}/Copying \
              $out/share/licenses/nes-deck/FCEUmm-COPYING
          '';
          description = "Rust Deck host with the pinned FCEUmm NES core";
          homepage = "https://github.com/libretro/libretro-fceumm";
          license = pkgs.lib.licenses.gpl2Only;
        };


        deck-menu = pkgsCross.stdenv.mkDerivation {
          pname = "deck-menu";
          version = "1.0.0";

          dontUnpack = true;
          nativeBuildInputs = [ pkgs.nukeReferences ] ++ waylandNativeInputs;
          buildInputs = [
            pkgsCross.glibc.static
            staticCross.libpng
            staticCross.zlib
          ] ++ waylandStaticInputs;
          allowedReferences = [ ];

          NIX_CFLAGS_COMPILE = "-static -Os";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            ${waylandProtocolBuild}
            cp ${menuSources}/deck_menu.cpp deck_menu.cpp
            cp ${menuSources}/menu_sound.cpp menu_sound.cpp
            cp ${menuSources}/menu_sound.h menu_sound.h
            cp ${menuSources}/menu_catalog.cpp menu_catalog.cpp
            cp ${menuSources}/menu_catalog.h menu_catalog.h
            cp ${menuSources}/menu_credits.cpp menu_credits.cpp
            cp ${menuSources}/menu_credits.h menu_credits.h
            cp ${menuSources}/menu_io.cpp menu_io.cpp
            cp ${menuSources}/menu_io.h menu_io.h
            cp ${menuSources}/menu_network.cpp menu_network.cpp
            cp ${menuSources}/menu_network.h menu_network.h
            cp ${menuSources}/menu_state.cpp menu_state.cpp
            cp ${menuSources}/menu_state.h menu_state.h
            cp ${menuSources}/menu_text.cpp menu_text.cpp
            cp ${menuSources}/menu_text.h menu_text.h
            cp ${menuSources}/menu_ui.cpp menu_ui.cpp
            cp ${menuSources}/menu_ui.h menu_ui.h
            $CXX -std=c++11 -Os -Wall -Wextra -Wpedantic -Werror \
              -DRETRO_DECK_WAYLAND=1 -I. -I${menuSources} \
              deck_menu.cpp menu_sound.cpp menu_catalog.cpp \
              menu_credits.cpp menu_io.cpp \
              menu_network.cpp menu_state.cpp menu_text.cpp menu_ui.cpp \
              ${menuSources}/deck_wayland.cpp \
              deck-widget-v1-protocol.o \
              wlr-layer-shell-unstable-v1-protocol.o \
              -static -lpng -lz -lwayland-client -lffi -o deck-menu
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin
            install -m755 deck-menu $out/bin/deck-menu
            nuke-refs $out/bin/deck-menu
            runHook postInstall
          '';

          meta = {
            description = "Touch-first game launcher for the Braiins Forge Deck";
            platforms = [ "armv7l-linux" ];
          };
        };

        chiptune-deck = pkgsCross.rustPlatform.buildRustPackage {
          pname = "chiptune-deck";
          version = "0.1.0";
          src = chiptuneRustSources;
          inherit cargoLock;
          cargoBuildFlags = [
            "-p"
            "retro-deck-apps"
            "--bin"
            "chiptune-deck"
            "--features"
            "chiptune-gme"
          ];
          doCheck = false;

          env.RUSTFLAGS = "-C target-feature=+crt-static -C link-arg=-static";
          nativeBuildInputs = [ pkgs.cmake pkgs.nukeReferences ];
          buildInputs = [ pkgsCross.glibc.static staticCross.zlib ];
          allowedReferences = [ ];

          preBuild = ''
            gme_source=$TMPDIR/game-music-emu
            gme_build=$TMPDIR/game-music-emu-build
            mkdir -p "$gme_source" "$gme_build"
            tar --extract --file=${pkgs.game-music-emu.src} \
              --directory="$gme_source" --strip-components=1
            chmod -R u+w "$gme_source"
            cmake -S "$gme_source" -B "$gme_build" \
              -DCMAKE_BUILD_TYPE=Release \
              -DCMAKE_C_COMPILER="$CC" \
              -DCMAKE_CXX_COMPILER="$CXX" \
              -DCMAKE_AR=${pkgsCross.stdenv.cc.bintools.bintools}/bin/${pkgsCross.stdenv.cc.targetPrefix}ar \
              -DCMAKE_RANLIB=${pkgsCross.stdenv.cc.bintools.bintools}/bin/${pkgsCross.stdenv.cc.targetPrefix}ranlib \
              -DBUILD_SHARED_LIBS=OFF \
              -DENABLE_UBSAN=OFF
            cmake --build "$gme_build" --parallel "$NIX_BUILD_CORES"
            export RUSTFLAGS="$RUSTFLAGS ${
              staticArchiveLinkFlags
                "$gme_build/gme/libgme.a"
                [ "-lstdc++" "-lz" ]
            }"
          '';

          postInstall = ''
            mkdir -p $out/bin $out/share/licenses/chiptune-deck
            tar --extract --file=${pkgs.game-music-emu.src} \
              --to-stdout \
              game-music-emu-${pkgs.game-music-emu.version}/license.txt \
              > $out/share/licenses/chiptune-deck/license.txt
          '';

          postFixup = ''
            ${pkgsCross.stdenv.cc.bintools.bintools}/bin/${pkgsCross.stdenv.cc.targetPrefix}strip \
              --strip-all $out/bin/chiptune-deck
            nuke-refs $out/bin/chiptune-deck
          '';

          meta = {
            description = "Rust chiptune music player for the Braiins Forge Deck";
            homepage = "https://github.com/libgme/game-music-emu";
            license = [
              pkgs.lib.licenses.gpl3Only
              pkgs.lib.licenses.lgpl21Plus
              pkgs.lib.licenses.bsd3
            ];
            platforms = [ "armv7l-linux" ];
          };
        };

        ten-seconds-deck = pkgsCross.rustPlatform.buildRustPackage {
          pname = "ten-seconds-deck";
          version = "0.1.0";
          src = timerRustSources;
          inherit cargoLock;
          cargoBuildFlags = [
            "-p"
            "retro-deck-apps"
            "--bin"
            "ten-seconds-deck"
          ];
          doCheck = false;

          env.RUSTFLAGS = "-C target-feature=+crt-static";
          nativeBuildInputs = [ pkgs.nukeReferences ];
          buildInputs = [ pkgsCross.glibc.static ];
          allowedReferences = [ ];

          postFixup = ''
            ${pkgsCross.stdenv.cc.bintools.bintools}/bin/${pkgsCross.stdenv.cc.targetPrefix}strip \
              --strip-all $out/bin/ten-seconds-deck
            nuke-refs $out/bin/ten-seconds-deck
          '';

          meta = {
            description = "Touch-controlled ten-second game for the Deck";
            platforms = [ "armv7l-linux" ];
          };
        };

        gb-deck = mkLibretroHost {
          pname = "gb-deck";
          version = "0.2.0";
          coreBuild = ''
            core=$TMPDIR/gambatte-core
            cp -R ${gambatte-src} "$core"
            chmod -R u+w "$core"
            patch_root=${./vendor/emulators/gambatte/patches}
            while IFS= read -r local_patch || [ -n "$local_patch" ]; do
              case "$local_patch" in
                ""|\#*) continue ;;
              esac
              patch -d "$core" -p1 < "$patch_root/$local_patch"
            done < "$patch_root/series"
            # Preserve Gambatte's include/feature flags while replacing its
            # generic release setting with safe Deck Cortex-A7 tuning.
            substituteInPlace "$core/Makefile.libretro" \
              --replace-fail \
                'CFLAGS   += -O2 -DNDEBUG' \
                'CFLAGS   += -O3 -fomit-frame-pointer -marm -march=armv7-a -mtune=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard -DNDEBUG' \
              --replace-fail \
                'CXXFLAGS += -O2 -DNDEBUG' \
                'CXXFLAGS += -O3 -fomit-frame-pointer -marm -march=armv7-a -mtune=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard -DNDEBUG'
            # Libretro's normal static build expects these utility symbols
            # from RetroArch.  This standalone frontend has no RetroArch, so
            # include the core's vendored implementations in its archive.
            substituteInPlace "$core/Makefile.common" \
              --replace-fail \
                'ifneq ($(STATIC_LINKING), 1)' \
                'ifeq ($(STATIC_LINKING), 1)'
            make -C "$core" -j$NIX_BUILD_CORES \
              STATIC_LINKING=1 \
              platform=unix \
              TARGET=gambatte_libretro.a \
              CC=$CC \
              CXX=$CXX \
              AR=${pkgsCross.stdenv.cc.targetPrefix}ar \
              fpic= \
              HAVE_NETWORK=0
          '';
          coreArchive = "$core/gambatte_libretro.a";
          nativeLibraries = [ "-lstdc++" ];
          extraNativeBuildInputs = [ pkgs.gnupatch ];
          installLicenses = ''
            install -m644 ${gambatte-src}/COPYING \
              $out/share/licenses/gb-deck/Gambatte-COPYING
          '';
          description = "Rust Deck host with the pinned Gambatte GB/GBC core";
          homepage = "https://github.com/libretro/gambatte-libretro";
          license = pkgs.lib.licenses.gpl2Only;
        };

        zx-deck = mkLibretroHost {
          pname = "zx-deck";
          version = "0.2.0";
          coreBuild = ''
            core=$TMPDIR/fuse-core
            cp -R ${fuse-src} "$core"
            chmod -R u+w "$core"
            patch_root=${./vendor/emulators/fuse/patches}
            while IFS= read -r local_patch || [ -n "$local_patch" ]; do
              case "$local_patch" in
                ""|\#*) continue ;;
              esac
              patch -d "$core" -p1 < "$patch_root/$local_patch"
            done < "$patch_root/series"
            # The Nix source has no Git metadata. Generate the version source
            # once from the pinned revision instead of invoking git.
            substituteInPlace "$core/Makefile.libretro" \
              --replace-fail \
                '$(CORE_DIR)/src/version.c: FORCE' \
                '$(CORE_DIR)/src/version.c:'
            sed 's/HASH/bce196fb774835fe65b3e5b821887a4ccf657167/' \
              "$core/etc/version.c.templ" > "$core/src/version.c"
            make -C "$core" -f Makefile.libretro -j$NIX_BUILD_CORES \
              platform=rpi2 \
              STATIC_LINKING=1 \
              TARGET=fuse_libretro.a \
              CC=$CC \
              CXX=$CXX \
              AR=${pkgsCross.stdenv.cc.targetPrefix}ar
          '';
          coreArchive = "$core/fuse_libretro.a";
          nativeLibraries = [ "-lstdc++" ];
          extraNativeBuildInputs = [ pkgs.gnupatch ];
          installLicenses = ''
            install -m644 ${fuse-src}/LICENSE \
              $out/share/licenses/zx-deck/Fuse-LICENSE
            install -m644 ${fuse-src}/libspectrum/COPYING \
              $out/share/licenses/zx-deck/libspectrum-COPYING
            install -m644 ${fuse-src}/bzip2/LICENSE \
              $out/share/licenses/zx-deck/bzip2-LICENSE
          '';
          description = "Rust Deck host with the pinned Fuse ZX Spectrum core";
          homepage = "https://github.com/libretro/fuse-libretro";
          license = pkgs.lib.licenses.gpl3Only;
        };

        chip8-deck = pkgsCross.rustPlatform.buildRustPackage {
          pname = "chip8-deck";
          version = "0.1.0";
          src = chip8RustSources;
          inherit cargoLock;
          cargoBuildFlags = [
            "-p"
            "retro-deck-emulator"
            "--bin"
            "chip8-deck"
          ];
          doCheck = false;

          env.RUSTFLAGS = "-C target-feature=+crt-static";
          nativeBuildInputs = [ pkgs.nukeReferences ];
          buildInputs = [ pkgsCross.glibc.static ];
          allowedReferences = [ ];

          postInstall = ''
            mkdir -p $out/bin $out/share/licenses/chip8-deck
            install -m644 vendor/emulators/c-octo/LICENSE.txt \
              $out/share/licenses/chip8-deck/c-octo-LICENSE
          '';

          postFixup = ''
            ${pkgsCross.stdenv.cc.bintools.bintools}/bin/${pkgsCross.stdenv.cc.targetPrefix}strip \
              --strip-all $out/bin/chip8-deck
            nuke-refs $out/bin/chip8-deck
          '';

          meta = {
            description = "Rust CHIP-8 host for the vendored c-octo core";
            homepage = "https://github.com/JohnEarnest/c-octo";
            license = [ pkgs.lib.licenses.gpl3Only pkgs.lib.licenses.mit ];
            platforms = [ "armv7l-linux" ];
          };
        };

        lua-deck = pkgsCross.stdenv.mkDerivation {
          pname = "lua-deck";
          version = "5.5.0";

          src = lua-src;
          nativeBuildInputs = [ pkgs.gnumake pkgs.nukeReferences ];
          buildInputs = [ pkgsCross.glibc.static ];
          allowedReferences = [ ];

          NIX_CFLAGS_COMPILE = "-Os";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            make -C src -j$NIX_BUILD_CORES posix \
              CC="$CC -std=gnu99" \
              AR="${pkgsCross.stdenv.cc.targetPrefix}ar rcu" \
              RANLIB=${pkgsCross.stdenv.cc.targetPrefix}ranlib \
              MYCFLAGS="-Os -ffunction-sections -fdata-sections" \
              MYLDFLAGS="-static -Wl,--gc-sections -Wl,-s"
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/lua-deck
            install -m755 src/lua $out/bin/lua
            install -m644 doc/readme.html \
              $out/share/licenses/lua-deck/LICENSE.html
            nuke-refs $out/bin/lua
            runHook postInstall
          '';

          meta = {
            description = "Static Lua interpreter for the Braiins Forge Deck";
            homepage = "https://www.lua.org/";
            license = pkgs.lib.licenses.mit;
            platforms = [ "armv7l-linux" ];
          };
        };

        python-deck = pkgsCross.stdenv.mkDerivation {
          pname = "python-deck";
          version = pkgs.micropython.version;

          src = pkgs.micropython.src;
          nativeBuildInputs = [ pkgs.gnumake pkgs.python3 pkgs.nukeReferences ];
          buildInputs = [ pkgsCross.glibc.static ];
          allowedReferences = [ ];

          NIX_CFLAGS_COMPILE = "-Os";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            make -C ports/unix -j$NIX_BUILD_CORES \
              VARIANT=minimal \
              CROSS_COMPILE=${pkgsCross.stdenv.cc.targetPrefix} \
              CC=$CC \
              STRIP=${pkgsCross.stdenv.cc.targetPrefix}strip \
              CFLAGS_EXTRA="-Os -ffunction-sections -fdata-sections" \
              LDFLAGS_EXTRA="-static -Wl,--gc-sections"
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/python-deck
            install -m755 ports/unix/build-minimal/micropython \
              $out/bin/python
            install -m644 LICENSE \
              $out/share/licenses/python-deck/LICENSE
            nuke-refs $out/bin/python
            runHook postInstall
          '';

          meta = {
            description = "Static MicroPython REPL for the Braiins Forge Deck";
            homepage = "https://micropython.org/";
            license = pkgs.lib.licenses.mit;
            platforms = [ "armv7l-linux" ];
          };
        };

        chibi-deck = pkgsCross.stdenv.mkDerivation {
          pname = "chibi-deck";
          version = pkgs.chibi.version;

          src = pkgs.chibi.src;
          patches = [ ./patches/chibi-static-module-path.patch ];
          nativeBuildInputs = [ pkgs.gnumake pkgs.chibi pkgs.nukeReferences ];
          buildInputs = [ pkgsCross.glibc.static ];
          allowedReferences = [ ];

          NIX_CFLAGS_COMPILE = "-Os";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            make -j$NIX_BUILD_CORES clibs.c \
              PLATFORM=linux \
              CHIBI_DEPENDENCIES= \
              CHIBI="${pkgs.chibi}/bin/chibi-scheme -I ./lib" \
              CHIBI_FFI="${pkgs.chibi}/bin/chibi-scheme -I ./lib -q tools/chibi-ffi"
            make -j$NIX_BUILD_CORES chibi-scheme-static \
              PLATFORM=linux \
              ARCH=armv7l \
              CC=$CC \
              AR=${pkgsCross.stdenv.cc.targetPrefix}ar \
              CPPFLAGS="-DSEXP_USE_DL=0 -DSEXP_USE_STATIC_LIBS=1 -DSEXP_USE_STATIC_LIBS_NO_INCLUDE=0" \
              CFLAGS="-Os -ffunction-sections -fdata-sections" \
              LDFLAGS="-static -Wl,--gc-sections -Wl,-s"
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/chibi \
              $out/share/licenses/chibi-deck
            install -m755 chibi-scheme-static $out/bin/chibi-scheme
            cp -R ${pkgs.chibi}/share/chibi/. $out/share/chibi/
            chmod -R u+w $out/share/chibi
            find $out/share/chibi -type f \
              \( -name '*.so' -o -name '*.img' \) -delete
            cd ${pkgs.chibi}/lib/chibi
            find . -type f -name '*.so' -print | while read module; do
              mkdir -p "$out/share/chibi/$(dirname "$module")"
              install -m444 /dev/null "$out/share/chibi/$module"
            done
            cd - >/dev/null
            install -m644 COPYING \
              $out/share/licenses/chibi-deck/COPYING
            nuke-refs $out/bin/chibi-scheme
            runHook postInstall
          '';

          meta = {
            description = "Static Chibi Scheme REPL for the Braiins Forge Deck";
            homepage = "https://github.com/ashinn/chibi-scheme";
            license = pkgs.lib.licenses.bsd3;
            platforms = [ "armv7l-linux" ];
          };
        };

        rlwrap-deck = staticCross.rlwrap.overrideAttrs (old: {
          pname = "rlwrap-deck";
          nativeBuildInputs = (old.nativeBuildInputs or []) ++
            [ pkgs.nukeReferences ];
          allowedReferences = [ ];

          postInstall = (old.postInstall or "") + ''
            rm -rf $out/share
            mkdir -p $out/share/licenses/rlwrap-deck
            install -m644 COPYING $out/share/licenses/rlwrap-deck/COPYING
          '';

          postFixup = (old.postFixup or "") + ''
            rm -rf $out/nix-support
            nuke-refs $out/bin/rlwrap
          '';

          meta = (old.meta or {}) // {
            description = "Static rlwrap for the Deck Lisp REPL";
            platforms = [ "armv7l-linux" ];
          };
        });

        fbterm-deck = staticCross.fbterm.overrideAttrs (old: {
          pname = "fbterm-deck";
          version = "1.7-deck";
          src = ./terminal/fbterm;

          # The Deck has no pointer-driven terminal UI, and static gpm in the
          # pinned nixpkgs leaves dangling shared-library symlinks.  Keep the
          # terminal fully static by disabling that optional integration.
          configureFlags = (old.configureFlags or []) ++ [ "--disable-gpm" ];
          nativeBuildInputs = (old.nativeBuildInputs or []) ++
            [ pkgs.nukeReferences ];
          propagatedBuildInputs = builtins.filter
            (dependency: dependency != staticCross.gpm)
            (old.propagatedBuildInputs or []);
          allowedReferences = [ ];

          postInstall = (old.postInstall or "") + ''
            mkdir -p $out/share/retro-deck/fonts \
              $out/share/retro-deck/keymaps \
              $out/share/licenses/fbterm-deck
            install -m755 ${staticCross.kbd}/bin/loadkeys $out/bin/loadkeys
            install -m644 \
              ${pkgs.dejavu_fonts}/share/fonts/truetype/DejaVuSansMono.ttf \
              $out/share/retro-deck/fonts/DejaVuSansMono.ttf
            ${pkgs.gzip}/bin/gzip -dc \
              ${pkgs.kbd}/share/keymaps/i386/qwerty/us.map.gz \
              > $out/share/retro-deck/keymaps/us.map
            ${pkgs.gzip}/bin/gzip -dc \
              ${pkgs.kbd}/share/keymaps/i386/qwertz/cz-qwertz.map.gz \
              > $out/share/retro-deck/keymaps/cz.map
            install -m644 \
              ${pkgs.kbd}/share/keymaps/i386/include/qwerty-layout.inc \
              ${pkgs.kbd}/share/keymaps/i386/include/compose.inc \
              ${pkgs.kbd}/share/keymaps/i386/include/linux-with-alt-and-altgr.inc \
              ${pkgs.kbd}/share/keymaps/i386/include/linux-keys-bare.inc \
              ${pkgs.kbd}/share/keymaps/include/compose.latin1 \
              $out/share/retro-deck/keymaps/
            ${pkgs.gzip}/bin/gzip -dc \
              ${pkgs.kbd}/share/keymaps/i386/include/euro1.map.gz \
              > $out/share/retro-deck/keymaps/euro1.map
            install -m644 COPYING $out/share/licenses/fbterm-deck/COPYING
            install -m644 ${./terminal/fonts/DejaVu-LICENSE} \
              $out/share/licenses/fbterm-deck/DejaVu-LICENSE
            tar --extract --xz --to-stdout --file=${pkgs.kbd.src} \
              --wildcards 'kbd-*/COPYING' \
              > $out/share/licenses/fbterm-deck/kbd-COPYING
          '';

          postFixup = (old.postFixup or "") + ''
            rm -rf $out/etc $out/nix-support/propagated-build-inputs
            nuke-refs $out/bin/fbterm $out/bin/loadkeys
          '';

          meta = (old.meta or {}) // {
            description = "Padded Deck fbterm with scoped US and Czech keymaps";
            platforms = [ "armv7l-linux" ];
          };
        });

        rom-uploader-host = pkgs.rustPlatform.buildRustPackage (uploaderPackage // {
          meta = {
            description = "Host-side Retro Deck uploader configuration helper";
            platforms = [ system ];
          };
        });

        rom-uploader = pkgsCross.rustPlatform.buildRustPackage (uploaderPackage // {
          env.RUSTFLAGS = "-C target-feature=+crt-static";
          nativeBuildInputs = [ pkgs.nukeReferences ];
          buildInputs = [ pkgsCross.glibc.static ];
          allowedReferences = [ ];

          postFixup = ''
            ${pkgsCross.stdenv.cc.bintools.bintools}/bin/${pkgsCross.stdenv.cc.targetPrefix}strip \
              --strip-all $out/bin/rom-uploader
            nuke-refs $out/bin/rom-uploader
          '';

          meta = {
            description = "Passworded ROM intake service for Retro Deck";
            platforms = [ "armv7l-linux" ];
          };
        });

        default = self.packages.${system}.nes-deck;
      };

      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = [
          pkgsCross.stdenv.cc
          pkgs.gnumake
        ];

        buildInputs = [
          pkgsCross.glibc.static
        ];

        shellHook = ''
          export CROSS_COMPILE="${pkgsCross.stdenv.cc.targetPrefix}"
          export CC="${pkgsCross.stdenv.cc}/bin/${pkgsCross.stdenv.cc.targetPrefix}gcc"
          export CXX="${pkgsCross.stdenv.cc}/bin/${pkgsCross.stdenv.cc.targetPrefix}g++"
          export CFLAGS="-static -O3 -fsigned-char"
          export LDFLAGS="-static -lpthread -lm"

          echo "Retro Deck cross-compile environment for Braiins Forge Deck"
          echo ""
          echo "Environment configured:"
          echo "  CROSS_COMPILE=$CROSS_COMPILE"
          echo "  CC=$CC"
          echo "  Target: armv7l-hf"
          echo ""
        '';
      };
    };
}
