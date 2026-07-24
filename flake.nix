{
  description = "Retro Deck emulators and launcher for Braiins Forge Deck";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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
    c-octo-src = {
      url = "github:JohnEarnest/c-octo/5f62f185c9e6ae324dcbe9e7fe35ec7c3bdebfb1";
      flake = false;
    };
    lua-src = {
      url = "https://www.lua.org/ftp/lua-5.5.0.tar.gz";
      flake = false;
    };
  };

  outputs =
    { self, nixpkgs, fenix, fceumm-src, gambatte-src, fuse-src, c-octo-src, lua-src }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      pkgsCross = pkgs.pkgsCross.armv7l-hf-multiplatform;
      staticCross = pkgs.pkgsCross.armv7l-hf-multiplatform.pkgsStatic;
      eclArm = import ./nix/ecl-arm-static.nix { };
      rustToolchain = fenix.packages.${system}.combine [
        fenix.packages.${system}.stable.cargo
        fenix.packages.${system}.stable.rustc
        fenix.packages.${system}.stable.rust-std
        fenix.packages.${system}.targets.armv7-unknown-linux-gnueabihf.stable.rust-std
      ];
      nativeSources = pkgs.lib.fileset.toSource {
        root = ./.;
        fileset = pkgs.lib.fileset.unions [
          ./native/Cargo.lock
          ./native/Cargo.toml
          ./native/src
          ./protocol/deck-widget-v1.xml
        ];
      };
      nativeCargoDeps = pkgs.rustPlatform.importCargoLock {
        lockFile = ./native/Cargo.lock;
      };

      waylandNativeInputs = [ pkgs.wayland-scanner ];
      waylandStaticInputs = [ staticCross.wayland staticCross.libffi ];
      # Keep each local build input narrow. Referencing ./src as an include
      # directory would make every source edit invalidate every native runtime.
      sourceTree = files: pkgs.lib.fileset.toSource {
        root = ./src;
        fileset = pkgs.lib.fileset.unions files;
      };
      runtimeSources = [
        ./src/deck_runtime.cpp
        ./src/deck_runtime.h
        ./src/deck_wayland.cpp
        ./src/deck_wayland.h
      ];
      libretroSources = runtimeSources ++ [
        ./src/joypad_input.cpp
        ./src/libretro_deck.cpp
      ];
      nesSources = sourceTree (libretroSources ++ [ ./src/nes_sram.h ]);
      gbSources = sourceTree libretroSources;
      zxSources = sourceTree (libretroSources ++ [ ./src/zx_keyboard.h ]);
      chip8Sources = sourceTree (runtimeSources ++ [
        ./src/chip8_core.c
        ./src/chip8_core.h
        ./src/chip8_deck.cpp
        ./src/joypad_input.cpp
      ]);
      chiptuneSources = sourceTree (runtimeSources ++ [
        ./src/chiptune_deck.cpp
      ]);
      timerSources = sourceTree (runtimeSources ++ [
        ./src/ten_seconds_deck.cpp
      ]);
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
        inherit pkgs pkgsCross staticCross nativeCargoDeps;
        nixpkgsSource = nixpkgs.outPath;
      };

    in
    {
      packages.${system} = {
        retrodeck-native = pkgs.stdenvNoCC.mkDerivation {
          pname = "retrodeck-native";
          version = "0.1.0";

          src = nativeSources;
          cargoDeps = nativeCargoDeps;
          cargoRoot = "native";
          nativeBuildInputs = [
            rustToolchain
            pkgs.rustPlatform.cargoSetupHook
            pkgsCross.stdenv.cc
            pkgs.nukeReferences
          ];
          buildInputs = [ pkgsCross.glibc.static ];
          allowedReferences = [ ];

          buildPhase = ''
            runHook preBuild
            cd native
            export CARGO_HOME=$TMPDIR/cargo
            export CARGO_BUILD_TARGET=armv7-unknown-linux-gnueabihf
            export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER="${pkgsCross.stdenv.cc}/bin/${pkgsCross.stdenv.cc.targetPrefix}cc"
            export RUSTFLAGS="\
              -C target-feature=+crt-static \
              -C link-arg=-static \
              -L native=${eclArm.dev}/lib \
              -L native=${pkgsCross.glibc.static}/lib \
              -l static=ecl \
              -l static=eclgc \
              -l static=gmp \
              -l dl \
              -l m"
            cargo build --release --locked --offline
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            install -Dm755 \
              target/armv7-unknown-linux-gnueabihf/release/retrodeck-native \
              $out/bin/retrodeck-native
            nuke-refs $out/bin/retrodeck-native
            runHook postInstall
          '';

          meta = {
            description = "Rust mechanism host for the RetroDeck Lisp orchestrator";
            platforms = [ system ];
          };
        };

        runtime-licenses = runtimeLicenses;

        nes-deck = pkgsCross.stdenv.mkDerivation {
          pname = "nes-deck";
          version = "0.1.0-20260714-deck";

          src = fceumm-src;
          nativeBuildInputs =
            [ pkgs.gnumake pkgs.nukeReferences ] ++ waylandNativeInputs;
          buildInputs = [
            pkgsCross.glibc.static
            staticCross.zlib
          ] ++ waylandStaticInputs;
          allowedReferences = [ ];

          NIX_CFLAGS_COMPILE = "-static -O3";
          NIX_LDFLAGS = "-static";

          postPatch = ''
            # A standalone static frontend needs the core's vendored libretro
            # utility implementations instead of symbols from RetroArch.
            substituteInPlace Makefile.common \
              --replace-fail \
                'ifneq ($(STATIC_LINKING), 1)' \
                'ifeq ($(STATIC_LINKING), 1)'
          '';

          buildPhase = ''
            runHook preBuild
            ${waylandProtocolBuild}
            make -j$NIX_BUILD_CORES \
              platform=rpi2 \
              STATIC_LINKING=1 \
              TARGET=fceumm_libretro.a \
              EXTERNAL_ZLIB=1 \
              CC=$CC \
              AR=${pkgsCross.stdenv.cc.targetPrefix}ar
            $CXX -std=c++11 -O3 -fomit-frame-pointer \
              -marm -march=armv7-a -mtune=cortex-a7 \
              -mfpu=neon-vfpv4 -mfloat-abi=hard \
              -Wall -Wextra -Wpedantic -Werror \
              -DRETRO_DECK_NES=1 -DRETRO_DECK_WAYLAND=1 \
              -I. -Isrc/drivers/libretro/libretro-common/include -I${nesSources} \
              ${nesSources}/libretro_deck.cpp \
              ${nesSources}/deck_runtime.cpp \
              ${nesSources}/deck_wayland.cpp \
              ${nesSources}/joypad_input.cpp \
              deck-widget-v1-protocol.o \
              wlr-layer-shell-unstable-v1-protocol.o \
              fceumm_libretro.a \
              -static -Wl,-s -pthread -lm -lz -lwayland-client -lffi \
              -o nes-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/nes-deck
            install -m755 nes-deck $out/bin/nes-deck
            install -m644 Copying $out/share/licenses/nes-deck/FCEUmm-COPYING
            nuke-refs $out/bin/nes-deck
            runHook postInstall
          '';

          meta = {
            description = "FCEUmm NES core with Deck-native framebuffer frontend";
            homepage = "https://github.com/libretro/libretro-fceumm";
            license = pkgs.lib.licenses.gpl2Only;
            platforms = [ "armv7l-linux" ];
          };
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

        chiptune-deck = pkgsCross.stdenv.mkDerivation {
          pname = "chiptune-deck";
          version = pkgs.game-music-emu.version;

          src = pkgs.game-music-emu.src;
          nativeBuildInputs =
            [ pkgs.cmake pkgs.nukeReferences ] ++ waylandNativeInputs;
          buildInputs = [
            pkgsCross.glibc.static
            staticCross.libvorbis
            staticCross.zlib
          ] ++ waylandStaticInputs;
          allowedReferences = [ ];

          cmakeFlags = [
            "-DBUILD_SHARED_LIBS=OFF"
            "-DENABLE_UBSAN=OFF"
          ];

          buildPhase = ''
            runHook preBuild
            cmake --build . --parallel $NIX_BUILD_CORES
            ${waylandProtocolBuild}
            $CXX -std=c++11 -Os -Wall -Wextra -Wpedantic -Werror \
              -DRETRO_DECK_WAYLAND=1 -I. -I${chiptuneSources} -I.. \
              ${chiptuneSources}/chiptune_deck.cpp \
              ${chiptuneSources}/deck_runtime.cpp \
              ${chiptuneSources}/deck_wayland.cpp \
              deck-widget-v1-protocol.o \
              wlr-layer-shell-unstable-v1-protocol.o \
              gme/libgme.a \
              -static -Wl,-s -pthread -lvorbisfile -lvorbis -logg -lm -lz \
              -lwayland-client -lffi \
              -o chiptune-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/chiptune-deck
            install -m755 chiptune-deck $out/bin/chiptune-deck
            install -m644 ../license.txt \
              $out/share/licenses/chiptune-deck/license.txt
            tar -xOf ${pkgs.libvorbis.src} \
              libvorbis-${pkgs.libvorbis.version}/COPYING \
              > $out/share/licenses/chiptune-deck/libvorbis-COPYING
            tar -xOf ${pkgs.libogg.src} \
              libogg-${pkgs.libogg.version}/COPYING \
              > $out/share/licenses/chiptune-deck/libogg-COPYING
            nuke-refs $out/bin/chiptune-deck
            runHook postInstall
          '';

          meta = {
            description = "Native chiptune music player for the Braiins Forge Deck";
            homepage = "https://github.com/libgme/game-music-emu";
            license = [ pkgs.lib.licenses.lgpl21Plus pkgs.lib.licenses.bsd3 ];
            platforms = [ "armv7l-linux" ];
          };
        };

        ten-seconds-deck = pkgsCross.stdenv.mkDerivation {
          pname = "ten-seconds-deck";
          version = "1.0.0";

          dontUnpack = true;
          nativeBuildInputs = [ pkgs.nukeReferences ] ++ waylandNativeInputs;
          buildInputs = [ pkgsCross.glibc.static ] ++ waylandStaticInputs;
          allowedReferences = [ ];

          NIX_CFLAGS_COMPILE = "-static -O3";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            ${waylandProtocolBuild}
            $CXX -std=c++11 -O3 -Wall -Wextra -Wpedantic -Werror \
              -DRETRO_DECK_WAYLAND=1 -I. -I${timerSources} \
              ${timerSources}/ten_seconds_deck.cpp \
              ${timerSources}/deck_runtime.cpp \
              ${timerSources}/deck_wayland.cpp \
              deck-widget-v1-protocol.o \
              wlr-layer-shell-unstable-v1-protocol.o \
              -static -pthread -lm -lwayland-client -lffi \
              -o ten-seconds-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin
            install -m755 ten-seconds-deck $out/bin/ten-seconds-deck
            nuke-refs $out/bin/ten-seconds-deck
            runHook postInstall
          '';

          meta = {
            description = "Touch-controlled ten-second game for the Deck";
            platforms = [ "armv7l-linux" ];
          };
        };

        gb-deck = pkgsCross.stdenv.mkDerivation {
          pname = "gb-deck";
          version = "0.5.0-20260703-deck";

          src = gambatte-src;
          nativeBuildInputs =
            [ pkgs.gnumake pkgs.nukeReferences ] ++ waylandNativeInputs;
          buildInputs = [ pkgsCross.glibc.static ] ++ waylandStaticInputs;
          allowedReferences = [ ];

          NIX_CFLAGS_COMPILE = "-static -O3";
          NIX_LDFLAGS = "-static";

          postPatch = ''
            # Preserve Gambatte's include/feature flags while replacing its
            # generic -O2 release setting with the Deck SoC tuning used by
            # the project's own Cortex-A7 targets.
            substituteInPlace Makefile.libretro \
              --replace-fail \
                'CFLAGS   += -O2 -DNDEBUG' \
                'CFLAGS   += -Ofast -flto -fuse-linker-plugin -fomit-frame-pointer -fno-math-errno -marm -march=armv7-a -mtune=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard -DNDEBUG' \
              --replace-fail \
                'CXXFLAGS += -O2 -DNDEBUG' \
                'CXXFLAGS += -Ofast -flto -fuse-linker-plugin -fomit-frame-pointer -fno-math-errno -marm -march=armv7-a -mtune=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard -DNDEBUG'
            # Libretro's normal static build expects these utility symbols
            # from RetroArch.  This standalone frontend has no RetroArch, so
            # include the core's vendored implementations in its archive.
            substituteInPlace Makefile.common \
              --replace-fail \
                'ifneq ($(STATIC_LINKING), 1)' \
                'ifeq ($(STATIC_LINKING), 1)'
          '';

          buildPhase = ''
            runHook preBuild
            ${waylandProtocolBuild}
            make \
              STATIC_LINKING=1 \
              platform=unix \
              TARGET=gambatte_libretro.a \
              CC=$CC \
              CXX=$CXX \
              AR=$CC-ar \
              fpic= \
              HAVE_NETWORK=0
            $CXX -std=c++11 -Ofast -flto -fuse-linker-plugin \
              -fomit-frame-pointer -marm -march=armv7-a -mtune=cortex-a7 \
              -mfpu=neon-vfpv4 -mfloat-abi=hard \
              -Wall -Wextra -Wpedantic -Werror \
              -I. -Ilibgambatte/libretro-common/include -I${gbSources} \
              -DRETRO_DECK_GB=1 -DRETRO_DECK_WAYLAND=1 \
              ${gbSources}/libretro_deck.cpp \
              ${gbSources}/deck_runtime.cpp \
              ${gbSources}/deck_wayland.cpp \
              ${gbSources}/joypad_input.cpp \
              deck-widget-v1-protocol.o \
              wlr-layer-shell-unstable-v1-protocol.o \
              gambatte_libretro.a \
              -static -pthread -lm -lwayland-client -lffi -o gb-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/gb-deck
            install -m755 gb-deck $out/bin/gb-deck
            install -m644 COPYING $out/share/licenses/gb-deck/Gambatte-COPYING
            nuke-refs $out/bin/gb-deck
            runHook postInstall
          '';

          meta = {
            description = "Gambatte GB/GBC core with Deck-native framebuffer frontend";
            homepage = "https://github.com/libretro/gambatte-libretro";
            license = pkgs.lib.licenses.gpl2Only;
            platforms = [ "armv7l-linux" ];
          };
        };

        zx-deck = pkgsCross.stdenv.mkDerivation {
          pname = "zx-deck";
          version = "1.6.0-20260420-deck";

          src = fuse-src;
          nativeBuildInputs =
            [ pkgs.gnumake pkgs.nukeReferences ] ++ waylandNativeInputs;
          buildInputs = [ pkgsCross.glibc.static ] ++ waylandStaticInputs;
          allowedReferences = [ ];

          NIX_CFLAGS_COMPILE = "-static -O3";
          NIX_LDFLAGS = "-static";

          postPatch = ''
            # The Nix source has no Git metadata. Generate the version source
            # once from the pinned revision instead of invoking git.
            substituteInPlace Makefile.libretro \
              --replace-fail \
                '$(CORE_DIR)/src/version.c: FORCE' \
                '$(CORE_DIR)/src/version.c:'
          '';

          buildPhase = ''
            runHook preBuild
            ${waylandProtocolBuild}
            sed 's/HASH/bce196fb774835fe65b3e5b821887a4ccf657167/' \
              etc/version.c.templ > src/version.c
            make -f Makefile.libretro -j$NIX_BUILD_CORES \
              platform=rpi2 \
              STATIC_LINKING=1 \
              TARGET=fuse_libretro.a \
              CC=$CC \
              CXX=$CXX \
              AR=$CC-ar
            cp ${zxSources}/libretro_deck.cpp deck_libretro_deck.cpp
            cp ${zxSources}/deck_runtime.cpp deck_runtime.cpp
            cp ${zxSources}/deck_wayland.cpp deck_wayland.cpp
            cp ${zxSources}/joypad_input.cpp deck_joypad_input.cpp
            $CXX -std=c++11 -O3 -fomit-frame-pointer \
              -marm -march=armv7-a -mtune=cortex-a7 \
              -mfpu=neon-vfpv4 -mfloat-abi=hard \
              -Wall -Wextra -Wpedantic -Werror \
              -DRETRO_DECK_ZX=1 -DRETRO_DECK_WAYLAND=1 \
              -I. -Isrc -I${zxSources} \
              deck_libretro_deck.cpp \
              deck_runtime.cpp \
              deck_wayland.cpp \
              deck_joypad_input.cpp \
              deck-widget-v1-protocol.o \
              wlr-layer-shell-unstable-v1-protocol.o \
              fuse_libretro.a \
              -static -pthread -lm -lwayland-client -lffi -o zx-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/zx-deck
            install -m755 zx-deck $out/bin/zx-deck
            install -m644 LICENSE $out/share/licenses/zx-deck/Fuse-LICENSE
            install -m644 libspectrum/COPYING \
              $out/share/licenses/zx-deck/libspectrum-COPYING
            install -m644 bzip2/LICENSE \
              $out/share/licenses/zx-deck/bzip2-LICENSE
            nuke-refs $out/bin/zx-deck
            runHook postInstall
          '';

          meta = {
            description = "Fuse ZX Spectrum core with Deck-native framebuffer frontend";
            homepage = "https://github.com/libretro/fuse-libretro";
            license = pkgs.lib.licenses.gpl3Only;
            platforms = [ "armv7l-linux" ];
          };
        };

        chip8-deck = pkgsCross.stdenv.mkDerivation {
          pname = "chip8-deck";
          version = "1.2-deck";

          dontUnpack = true;
          nativeBuildInputs = [ pkgs.nukeReferences ] ++ waylandNativeInputs;
          buildInputs = [ pkgsCross.glibc.static ] ++ waylandStaticInputs;
          allowedReferences = [ ];

          NIX_CFLAGS_COMPILE = "-static -O3";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            ${waylandProtocolBuild}
            cp ${chip8Sources}/chip8_core.c deck_chip8_core.c
            cp ${chip8Sources}/chip8_deck.cpp deck_chip8_deck.cpp
            cp ${chip8Sources}/deck_runtime.cpp deck_runtime.cpp
            cp ${chip8Sources}/deck_wayland.cpp deck_wayland.cpp
            cp ${chip8Sources}/joypad_input.cpp deck_joypad_input.cpp
            $CC -std=c99 -O3 -Wall -Wextra -Werror \
              -I${c-octo-src}/src -I${chip8Sources} \
              -c deck_chip8_core.c -o chip8_core.o
            $CXX -std=c++11 -O3 -Wall -Wextra -Wpedantic -Werror \
              -DRETRO_DECK_WAYLAND=1 -I. -I${chip8Sources} \
              deck_chip8_deck.cpp \
              deck_runtime.cpp \
              deck_wayland.cpp \
              deck_joypad_input.cpp \
              deck-widget-v1-protocol.o \
              wlr-layer-shell-unstable-v1-protocol.o \
              chip8_core.o -static -pthread -lm -lwayland-client -lffi \
              -o chip8-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/chip8-deck
            install -m755 chip8-deck $out/bin/chip8-deck
            install -m644 ${c-octo-src}/LICENSE.txt \
              $out/share/licenses/chip8-deck/c-octo-LICENSE
            nuke-refs $out/bin/chip8-deck
            runHook postInstall
          '';

          meta = {
            description = "c-octo CHIP-8/SCHIP/XO-CHIP core with Deck-native frontend";
            homepage = "https://github.com/JohnEarnest/c-octo";
            license = pkgs.lib.licenses.mit;
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

        rom-uploader = pkgsCross.buildGoModule {
          pname = "rom-uploader";
          version = "1.0.0";

          src = ./uploader;
          vendorHash = null;
          env.CGO_ENABLED = 0;
          nativeBuildInputs = [ pkgs.nukeReferences ];
          allowedReferences = [ ];
          ldflags = [ "-s" "-w" ];

          postInstall = ''
            mv $out/bin/uploader $out/bin/rom-uploader
          '';

          postFixup = ''
            nuke-refs $out/bin/rom-uploader
          '';

          meta = {
            description = "Passworded ROM intake service for Retro Deck";
            platforms = [ "armv7l-linux" ];
          };
        };

        default = self.packages.${system}.nes-deck;
      };

      checks.${system}.retrodeck-native-smoke =
        pkgs.runCommand "retrodeck-native-smoke" { } ''
          cp ${./lisp/startup.lisp} startup.lisp
          cp ${./lisp/ui.lisp} ui.lisp
          cp ${./lisp/policy.lisp} policy.lisp
          cp ${./lisp/process.lisp} process.lisp
          cp ${./lisp/settings.lisp} settings.lisp
          cp ${./lisp/wifi.lisp} wifi.lisp
          cp ${./lisp/credits.lisp} credits.lisp
          cp ${./lisp/dashboard.lisp} dashboard.lisp
          cp ${./assets/settings-cog/gear-knekko-09.png} settings-icon.png
          cat > terminal-fixture <<'EOF'
          #!${pkgs.runtimeShell}
          [ "$#" -eq 1 ] || exit 90
          [ "$RETRO_DECK_KEYMAP" = cz ] || exit 92
          case "$1" in
            shell) exit 0 ;;
            failure) exit 7 ;;
            signal) kill -TERM "$$" ;;
            *) exit 91 ;;
          esac
          EOF
          chmod +x terminal-fixture
          cat > helper-fixture <<'EOF'
          #!${pkgs.runtimeShell}
          [ "$#" -eq 0 ] || exit 90
          cat > helper-capture
          EOF
          cat > helper-failure-fixture <<'EOF'
          #!${pkgs.runtimeShell}
          [ "$#" -eq 0 ] || exit 90
          cat >/dev/null
          exit 7
          EOF
          cat > helper-signal-fixture <<'EOF'
          #!${pkgs.runtimeShell}
          [ "$#" -eq 0 ] || exit 90
          cat >/dev/null
          kill -TERM "$$"
          EOF
          cat > helper-reject-fixture <<'EOF'
          #!${pkgs.runtimeShell}
          [ "$#" -eq 0 ] || exit 90
          exit 7
          EOF
          chmod +x helper-fixture helper-failure-fixture \
            helper-signal-fixture helper-reject-fixture
          printf 'CONNECTED\n' > wifi-status
          rm -f helper-capture state-file state-file.keymap state-file.control \
            state-file.brightness state-file.brightness-max \
            state-file.brightness-state
          printf '12\n' > state-file.control
          printf '12\n' > state-file.brightness
          printf '20\n' > state-file.brightness-max
          substitute ${./tests/native_ecl_smoke.lisp.in} smoke.lisp \
            --subst-var-by startup "$PWD/startup.lisp" \
            --subst-var-by state_file "$PWD/state-file" \
            --subst-var-by wifi_status "$PWD/wifi-status" \
            --subst-var-by settings_icon "$PWD/settings-icon.png" \
            --subst-var-by terminal_fixture "$PWD/terminal-fixture" \
            --subst-var-by helper_fixture "$PWD/helper-fixture" \
            --subst-var-by helper_failure_fixture "$PWD/helper-failure-fixture" \
            --subst-var-by helper_signal_fixture "$PWD/helper-signal-fixture" \
            --subst-var-by helper_reject_fixture "$PWD/helper-reject-fixture" \
            --subst-var-by helper_capture "$PWD/helper-capture" \
            --subst-var-by credits ${./deploy/menu/credits.tsv}
          RETRO_DECK_VOLUME_PERCENT=0 \
            ECLDIR=${eclArm}/lib/ecl/ \
            ${pkgs.qemu-user}/bin/qemu-arm \
            ${self.packages.${system}.retrodeck-native}/bin/retrodeck-native \
            smoke.lisp
          touch $out
        '';

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
