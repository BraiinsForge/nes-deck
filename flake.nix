{
  description = "Retro Deck emulators and launcher for Braiins Forge Deck";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    infones-src = {
      url = "github:nejidev/arm-NES-linux";
      flake = false;
    };
    gambatte-src = {
      url = "github:libretro/gambatte-libretro/dfc165599f3f1068c40a0b7ad6fe5f161283d483";
      flake = false;
    };
    c-octo-src = {
      url = "github:JohnEarnest/c-octo/5f62f185c9e6ae324dcbe9e7fe35ec7c3bdebfb1";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, infones-src, gambatte-src, c-octo-src }:
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

    in
    {
      packages.${system} = {
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

          NIX_CFLAGS_COMPILE = "-static -O3 -fsigned-char";
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

            # Create Makefile for static build
            cat > linux/Makefile << 'MAKEFILE_EOF'
            CROSS_COMPILE ?=
            CC = $(CROSS_COMPILE)gcc
            CXX = $(CROSS_COMPILE)g++
            CFLAGS = -O3 -fsigned-char -DNDEBUG
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
          buildInputs = [ pkgsCross.glibc.static ];

          NIX_CFLAGS_COMPILE = "-static -Os";
          NIX_LDFLAGS = "-static";

          buildPhase = ''
            runHook preBuild
            $CXX -std=c++11 -Os -Wall -Wextra -Wpedantic -Werror \
              ${./src/deck_menu.cpp} -static -o deck-menu
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
              ${./src/gb_deck.cpp} \
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

        default = self.packages.${system}.infones-deck;
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

          echo "InfoNES cross-compile environment for Braiins Forge Deck"
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
