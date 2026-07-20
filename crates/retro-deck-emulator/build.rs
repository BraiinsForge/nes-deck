#[cfg(feature = "chip8")]
use std::path::PathBuf;

#[cfg(feature = "chip8")]
fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let adapter = manifest.join("native/c_octo_adapter.c");
    let upstream = manifest.join("../../vendor/emulators/c-octo/upstream");
    let header = upstream.join("octo_emulator.h");

    println!("cargo:rerun-if-changed={}", adapter.display());
    println!("cargo:rerun-if-changed={}", header.display());

    cc::Build::new()
        .file(adapter)
        .include(upstream)
        .std("c99")
        .warnings(true)
        .warnings_into_errors(true)
        .compile("retro_deck_c_octo");
}

#[cfg(not(feature = "chip8"))]
fn main() {}
