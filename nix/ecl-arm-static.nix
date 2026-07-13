{
  nixpkgsSrc ? builtins.fetchTarball {
    # nixpkgs revision 767b0d3ec98a143ad9ed7dfc0d5553510ac27133
    url = "https://releases.nixos.org/nixpkgs/nixpkgs-26.11pre1031701.767b0d3ec98a/nixexprs.tar.xz";
    sha256 = "sha256-E/v/PHozqkEfjEy5iyvJJ+aQxgH+XV6hOjle67HQ+P4=";
  },
}:

let
  nixpkgsRev = "767b0d3ec98a143ad9ed7dfc0d5553510ac27133";
  system = "x86_64-linux";
  pkgs = import nixpkgsSrc { inherit system; };
  cross = pkgs.pkgsCross.armv7l-hf-multiplatform;

  gmpStatic = cross.gmp.override { withStatic = true; };
  buildEcl = cross.buildPackages.ecl;

  eclStatic = (cross.ecl.override {
    gmp = gmpStatic;
    threadSupport = false;
  }).overrideAttrs (old: {
    nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ buildEcl ];
    buildInputs = (old.buildInputs or [ ]) ++ [ cross.stdenv.cc.libc.static ];
    propagatedBuildInputs = [ gmpStatic ];

    ECL_TO_RUN = "${buildEcl}/bin/ecl";
    configureFlags = (old.configureFlags or [ ]) ++ [
      "--with-cross-config=../src/util/x86-linux-gnu.cross_config"
      "--disable-shared"
      "--disable-threads"
      "--enable-boehm=included"
      "--enable-gmp=system"
      "--enable-manual=no"
      "--with-cmp=no"
      "--with-bytecmp=builtin"
      "--with-asdf=builtin"
      "--with-tcp=no"
      "--with-serve-event=no"
      "--with-clos-streams=yes"
      "--with-cmuformat=yes"
      "--with-dffi=no"
    ];

    NIX_CFLAGS_COMPILE = "-Os -ffunction-sections -fdata-sections";
    LDFLAGS = "-static";
    NIX_LDFLAGS = "--gc-sections";
    dontDisableStatic = true;

    # The cross-built executable cannot run during the x86_64 build, and the
    # nixpkgs post-install hook would wrap it with target build-tool paths.
    doInstallCheck = false;
    postInstall = "";
  });

  version = eclStatic.version;
  targetPrefix = cross.stdenv.cc.targetPrefix;
  targetBinutils = cross.stdenv.cc.bintools;
in
assert pkgs.lib.assertMsg (version == "26.5.5")
  "ecl-arm-static.nix expects ECL 26.5.5 from nixpkgs ${nixpkgsRev}";
pkgs.runCommand "ecl-arm-static-runtime-${version}" {
  inherit version;
  nativeBuildInputs = [ pkgs.nukeReferences ];

  # The payload must remain independent of its build-time ECL/glibc closure.
  allowedReferences = [ ];

  passthru = {
    eclVersion = version;
    inherit nixpkgsRev;
    eclDir = "lib/ecl/";
    targetSystem = "armv7l-linux";
  };

  meta = {
    description = "Minimal static ECL runtime for the ARMv7 Forge Deck";
    # This derivation runs on the cross-build host; its payload targets ARMv7.
    platforms = [ system ];
  };
} ''
  install -Dm755 ${eclStatic}/bin/ecl $out/bin/ecl.bin
  install -Dm444 ${eclStatic}/lib/ecl-${version}/help.doc \
    $out/lib/ecl/help.doc

  ${targetBinutils}/bin/${targetPrefix}strip --strip-all $out/bin/ecl.bin

  # Static glibc embeds build-host locale/gconv search paths. They are inert on
  # the Deck, but scrubbing them keeps the Nix runtime closure truly empty.
  nuke-refs $out/bin/ecl.bin
''
