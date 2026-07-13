{
  description = "InfoNES for Braiins Forge Deck (armv7 static framebuffer build)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    infones-src = {
      url = "github:nejidev/arm-NES-linux";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, infones-src }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      pkgsCross = pkgs.pkgsCross.armv7l-hf-multiplatform;

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
