//! Host-side PPM capture from the production Rust dashboard renderer.

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufWriter, Write as _};
use std::path::Path;
use std::process::ExitCode;

use retro_deck_config::{Catalog, Palette};
use retro_deck_dashboard::{
    Brightness, CANVAS_HEIGHT, CANVAS_WIDTH, DashboardCatalog, DashboardFrame, DashboardModel,
    Keymap, VolumeState,
};

const APPLICATION: &str = "render-dashboard";
const CATALOG: &[u8] = include_bytes!("../../../../deploy/menu/games.tsv");
const PALETTE: &[u8] = include_bytes!("../../../../deploy/menu/palette.tsv");

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let program = arguments
        .next()
        .unwrap_or_else(|| OsString::from(APPLICATION));
    let Some(output) = arguments.next() else {
        eprintln!("Usage: {} OUTPUT.ppm", Path::new(&program).display());
        return ExitCode::from(2);
    };
    if arguments.next().is_some() {
        eprintln!("Usage: {} OUTPUT.ppm", Path::new(&program).display());
        return ExitCode::from(2);
    }

    match render_preview(Path::new(&output)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn render_preview(output: &Path) -> Result<(), Box<dyn Error>> {
    let catalog = Catalog::parse(CATALOG)?;
    let catalog = DashboardCatalog::from_catalog(&catalog)?;
    let model = DashboardModel::new(
        catalog,
        VolumeState::new(42, 42)?,
        Brightness::new(60)?,
        Keymap::Us,
    );
    let frame = DashboardFrame::render_menu(&model, &Palette::parse_tsv(PALETTE)?)?;
    write_ppm(output, frame.pixels())?;
    Ok(())
}

fn write_ppm(path: &Path, pixels: &[u16]) -> io::Result<()> {
    if pixels.len() != CANVAS_WIDTH.saturating_mul(CANVAS_HEIGHT) {
        return Err(io::Error::other(
            "dashboard preview pixel count does not match",
        ));
    }
    let file = File::create(path)?;
    let mut output = BufWriter::new(file);
    write!(output, "P6\n{CANVAS_WIDTH} {CANVAS_HEIGHT}\n255\n")?;
    let row_bytes = CANVAS_WIDTH
        .checked_mul(3)
        .ok_or_else(|| io::Error::other("dashboard preview row size overflowed"))?;
    let mut encoded = vec![0_u8; row_bytes];
    for row in pixels.chunks_exact(CANVAS_WIDTH) {
        for (output_pixel, pixel) in encoded.chunks_exact_mut(3).zip(row.iter().copied()) {
            output_pixel.copy_from_slice(&expand_rgb565(pixel));
        }
        output.write_all(&encoded)?;
    }
    output.flush()
}

fn expand_rgb565(pixel: u16) -> [u8; 3] {
    let red = u8::try_from((pixel >> 11) & 0x1f).unwrap_or_default();
    let green = u8::try_from((pixel >> 5) & 0x3f).unwrap_or_default();
    let blue = u8::try_from(pixel & 0x1f).unwrap_or_default();
    [
        (red << 3) | (red >> 2),
        (green << 2) | (green >> 4),
        (blue << 3) | (blue >> 2),
    ]
}

#[cfg(test)]
mod tests {
    use super::expand_rgb565;

    #[test]
    fn rgb565_expansion_preserves_primary_endpoints() {
        assert_eq!(expand_rgb565(0x0000), [0, 0, 0]);
        assert_eq!(expand_rgb565(0xf800), [255, 0, 0]);
        assert_eq!(expand_rgb565(0x07e0), [0, 255, 0]);
        assert_eq!(expand_rgb565(0x001f), [0, 0, 255]);
        assert_eq!(expand_rgb565(0xffff), [255, 255, 255]);
    }
}
