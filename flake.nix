{
  description = "Retro Deck emulators and launcher for Braiins Forge Deck";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    infones-src = {
      url = "github:nejidev/arm-NES-linux";
      flake = false;
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
    { self, nixpkgs, infones-src, fceumm-src, gambatte-src, fuse-src, c-octo-src, lua-src }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      pkgsCross = pkgs.pkgsCross.armv7l-hf-multiplatform;
      staticCross = pkgs.pkgsCross.armv7l-hf-multiplatform.pkgsStatic;

      # Read our deck-specific source files
      deckSystemSrc = builtins.readFile ./src/InfoNES_System_Deck.cpp;
      joypadSrc = builtins.readFile ./src/joypad_input.cpp;
      audioMixerSrc = builtins.readFile ./src/nes_audio_mixer.h;
      apuNoiseSrc = builtins.readFile ./src/nes_apu_noise.h;
      nesSramSrc = builtins.readFile ./src/nes_sram.h;

    in
    {
      packages.${system} = {
        nes-deck = pkgsCross.stdenv.mkDerivation {
          pname = "nes-deck";
          version = "0.1.0-20260714-deck";

          src = fceumm-src;
          nativeBuildInputs = [ pkgs.gnumake ];
          buildInputs = [
            pkgsCross.glibc.static
            staticCross.zlib
          ];

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
              -DRETRO_DECK_NES=1 \
              -Isrc/drivers/libretro/libretro-common/include -I${./src} \
              ${./src/libretro_deck.cpp} \
              ${./src/deck_runtime.cpp} \
              ${./src/joypad_input.cpp} \
              fceumm_libretro.a \
              -static -Wl,-s -pthread -lm -lz -o nes-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/nes-deck
            install -m755 nes-deck $out/bin/nes-deck
            install -m644 Copying $out/share/licenses/nes-deck/FCEUmm-COPYING
            runHook postInstall
          '';

          meta = {
            description = "FCEUmm NES core with Deck-native framebuffer frontend";
            homepage = "https://github.com/libretro/libretro-fceumm";
            license = pkgs.lib.licenses.gpl2Only;
            platforms = [ "armv7l-linux" ];
          };
        };

        infones-deck = pkgsCross.stdenv.mkDerivation {
          pname = "infones-deck";
          version = "0.91j-deck";

          src = infones-src;
          patches = [
            ./patches/infones-apu-register.patch
            ./patches/infones-apu.patch
            ./patches/infones-apu-quality.patch
            ./patches/infones-apu-noise.patch
          ];

          nativeBuildInputs = [ pkgs.gnumake ];
          buildInputs = [ pkgsCross.glibc.static ];

          NIX_CFLAGS_COMPILE = "-static -O3 -fsigned-char -fomit-frame-pointer -marm -march=armv7-a -mtune=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard";
          NIX_LDFLAGS = "-static";

          # The pinned upstream file uses CRLF; normalize it so our focused
          # source patch applies reproducibly with Nix's patch phase.
          prePatch = ''
            sed -i 's/\r$//' InfoNES.cpp K6502_rw.h InfoNES_pAPU.cpp \
              InfoNES_pAPU.h \
              mapper/InfoNES_Mapper_000.cpp
          '';

          # Patch for Deck framebuffer support
          postPatch = ''
            # Install Deck-specific system file
            cat > linux/InfoNES_System_Linux.cpp << 'DECK_SYS_EOF'
            ${deckSystemSrc}
            DECK_SYS_EOF

            # Install TTY input handler
            cat > linux/joypad_input.cpp << 'JOYPAD_EOF'
            ${joypadSrc}
            JOYPAD_EOF

            # Install the small, host-testable mixer/resampler helper
            cat > linux/nes_audio_mixer.h << 'AUDIO_MIXER_EOF'
            ${audioMixerSrc}
            AUDIO_MIXER_EOF

            # Install host-tested helpers used by the patched noise channel
            cat > linux/nes_apu_noise.h << 'APU_NOISE_EOF'
            ${apuNoiseSrc}
            APU_NOISE_EOF

            # Install the tested battery-backed SRAM codec
            cat > linux/nes_sram.h << 'NES_SRAM_EOF'
            ${nesSramSrc}
            NES_SRAM_EOF

            # Create Makefile for static build
            cat > linux/Makefile << 'MAKEFILE_EOF'
            CROSS_COMPILE ?=
            CC = $(CROSS_COMPILE)gcc
            CXX = $(CROSS_COMPILE)g++
            CFLAGS = -O3 -fsigned-char -fomit-frame-pointer -marm \
                     -march=armv7-a -mtune=cortex-a7 \
                     -mfpu=neon-vfpv4 -mfloat-abi=hard -DNDEBUG
            CXXFLAGS = $(CFLAGS)
            LDFLAGS = -static -lpthread -lm

            OBJS = ../K6502.o ../InfoNES.o ../InfoNES_Mapper.o ../InfoNES_pAPU.o \
                   InfoNES_System_Linux.o joypad_input.o

            TARGET = InfoNES

            all: $(TARGET)

            $(TARGET): $(OBJS)
            	$(CXX) $(CXXFLAGS) -o $@ $(OBJS) $(LDFLAGS)

            %.o: %.cpp
            	$(CXX) $(CXXFLAGS) -c -o $@ $<

            ../%.o: ../%.cpp
            	$(CXX) $(CXXFLAGS) -c -o $@ $<

            clean:
            	rm -f $(OBJS) $(TARGET)
            MAKEFILE_EOF
          '';

          buildPhase = ''
            runHook preBuild
            cd linux
            make CROSS_COMPILE=${pkgsCross.stdenv.cc.targetPrefix}
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin
            cp InfoNES $out/bin/infones
            runHook postInstall
          '';

          meta = {
            description = "InfoNES - NES emulator for Braiins Forge Deck framebuffer";
            homepage = "https://github.com/nejidev/arm-NES-linux";
            platforms = [ "armv7l-linux" ];
          };
        };

        deck-menu = pkgsCross.stdenv.mkDerivation {
          pname = "deck-menu";
          version = "1.0.0";

          dontUnpack = true;
          buildInputs = [
            pkgsCross.glibc.static
            staticCross.libpng
            staticCross.zlib
          ];

          NIX_CFLAGS_COMPILE = "-static -Os";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            $CXX -std=c++11 -Os -Wall -Wextra -Wpedantic -Werror \
              ${./src/deck_menu.cpp} -static -lpng -lz -o deck-menu
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin
            install -m755 deck-menu $out/bin/deck-menu
            runHook postInstall
          '';

          meta = {
            description = "Touch-first game launcher for the Braiins Forge Deck";
            platforms = [ "armv7l-linux" ];
          };
        };

        ten-seconds-deck = pkgsCross.stdenv.mkDerivation {
          pname = "ten-seconds-deck";
          version = "1.0.0";

          dontUnpack = true;
          buildInputs = [ pkgsCross.glibc.static ];

          NIX_CFLAGS_COMPILE = "-static -O3";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            $CXX -std=c++11 -O3 -Wall -Wextra -Wpedantic -Werror \
              -I${./src} \
              ${./src/ten_seconds_deck.cpp} \
              ${./src/deck_runtime.cpp} \
              -static -pthread -lm -o ten-seconds-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin
            install -m755 ten-seconds-deck $out/bin/ten-seconds-deck
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
          nativeBuildInputs = [ pkgs.gnumake ];
          buildInputs = [ pkgsCross.glibc.static ];

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
              -Ilibgambatte/libretro-common/include -I${./src} \
              -DRETRO_DECK_GB=1 \
              ${./src/libretro_deck.cpp} \
              ${./src/deck_runtime.cpp} \
              ${./src/joypad_input.cpp} \
              gambatte_libretro.a \
              -static -pthread -lm -o gb-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/gb-deck
            install -m755 gb-deck $out/bin/gb-deck
            install -m644 COPYING $out/share/licenses/gb-deck/Gambatte-COPYING
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
          nativeBuildInputs = [ pkgs.gnumake ];
          buildInputs = [ pkgsCross.glibc.static ];

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
            sed 's/HASH/bce196fb774835fe65b3e5b821887a4ccf657167/' \
              etc/version.c.templ > src/version.c
            make -f Makefile.libretro -j$NIX_BUILD_CORES \
              platform=rpi2 \
              STATIC_LINKING=1 \
              TARGET=fuse_libretro.a \
              CC=$CC \
              CXX=$CXX \
              AR=$CC-ar
            $CXX -std=c++11 -O3 -fomit-frame-pointer \
              -marm -march=armv7-a -mtune=cortex-a7 \
              -mfpu=neon-vfpv4 -mfloat-abi=hard \
              -Wall -Wextra -Wpedantic -Werror \
              -DRETRO_DECK_ZX=1 \
              -Isrc -I${./src} \
              ${./src/libretro_deck.cpp} \
              ${./src/deck_runtime.cpp} \
              ${./src/joypad_input.cpp} \
              fuse_libretro.a \
              -static -pthread -lm -o zx-deck
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
          buildInputs = [ pkgsCross.glibc.static ];

          NIX_CFLAGS_COMPILE = "-static -O3";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            $CC -std=c99 -O3 -Wall -Wextra -Werror \
              -I${c-octo-src}/src -I${./src} \
              -c ${./src/chip8_core.c} -o chip8_core.o
            $CXX -std=c++11 -O3 -Wall -Wextra -Wpedantic -Werror \
              -I${./src} \
              ${./src/chip8_deck.cpp} \
              ${./src/deck_runtime.cpp} \
              ${./src/joypad_input.cpp} \
              chip8_core.o -static -pthread -lm -o chip8-deck
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/share/licenses/chip8-deck
            install -m755 chip8-deck $out/bin/chip8-deck
            install -m644 ${c-octo-src}/LICENSE.txt \
              $out/share/licenses/chip8-deck/c-octo-LICENSE
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

        fbterm-deck = staticCross.fbterm.overrideAttrs (old: {
          pname = "fbterm-deck";
          version = "1.7-deck";
          src = ./terminal/fbterm;

          # The Deck has no pointer-driven terminal UI, and static gpm in the
          # pinned nixpkgs leaves dangling shared-library symlinks.  Keep the
          # terminal fully static by disabling that optional integration.
          configureFlags = (old.configureFlags or []) ++ [ "--disable-gpm" ];
          propagatedBuildInputs = builtins.filter
            (dependency: dependency != staticCross.gpm)
            (old.propagatedBuildInputs or []);

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

          meta = (old.meta or {}) // {
            description = "Padded Deck fbterm with scoped US and Czech keymaps";
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
