//! Host-side exact-screen renderer for the 10 Seconds design capture.

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufWriter, Write as _};
use std::path::Path;
use std::process::ExitCode;

use retro_deck_apps::ten_seconds::{
    CANVAS_HEIGHT, CANVAS_WIDTH, InputSource, MonotonicNanoseconds, TimerFrame, TimerGame,
};
use retro_deck_platform::display::{
    DECK_DIMENSIONS, Dimensions, Frame, ScalePlan, gameplay_dimensions,
};

const APPLICATION: &str = "render-ten-seconds";
const PREVIEW_STOP_NANOSECONDS: u64 = 10_030_000_000;
const BLACK: u32 = 0xff00_0000;

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
    let source_dimensions = Dimensions::new(CANVAS_WIDTH, CANVAS_HEIGHT)
        .ok_or_else(|| io::Error::other("timer canvas dimensions are invalid"))?;
    let target_dimensions = gameplay_dimensions(source_dimensions)?;

    let mut game = TimerGame::new();
    let _ = game.activate(MonotonicNanoseconds::new(0), InputSource::Touch);
    let _ = game.activate(
        MonotonicNanoseconds::new(PREVIEW_STOP_NANOSECONDS),
        InputSource::Touch,
    );
    let timer = TimerFrame::render(game.view())?;

    let source = Frame::rgb565(timer.pixels(), source_dimensions, CANVAS_WIDTH)?;
    let scale = ScalePlan::new(source_dimensions, target_dimensions);
    let mut gameplay = vec![BLACK; target_dimensions.pixel_count()];
    scale.blit(source, &mut gameplay)?;

    let mut screen = vec![BLACK; DECK_DIMENSIONS.pixel_count()];
    copy_centered(&gameplay, target_dimensions, &mut screen, DECK_DIMENSIONS)?;
    write_ppm(output, &screen, DECK_DIMENSIONS)?;
    Ok(())
}

fn copy_centered(
    source: &[u32],
    source_dimensions: Dimensions,
    destination: &mut [u32],
    destination_dimensions: Dimensions,
) -> io::Result<()> {
    if source.len() != source_dimensions.pixel_count()
        || destination.len() != destination_dimensions.pixel_count()
        || source_dimensions.width() > destination_dimensions.width()
        || source_dimensions.height() > destination_dimensions.height()
    {
        return Err(io::Error::other("preview frame dimensions do not match"));
    }
    let left = (destination_dimensions.width() - source_dimensions.width()) / 2;
    let top = (destination_dimensions.height() - source_dimensions.height()) / 2;
    for (y, source_row) in source.chunks_exact(source_dimensions.width()).enumerate() {
        let destination_start = top
            .checked_add(y)
            .and_then(|row| row.checked_mul(destination_dimensions.width()))
            .and_then(|row| row.checked_add(left))
            .ok_or_else(|| io::Error::other("preview row offset overflowed"))?;
        let destination_end = destination_start
            .checked_add(source_dimensions.width())
            .ok_or_else(|| io::Error::other("preview row extent overflowed"))?;
        let destination_row = destination
            .get_mut(destination_start..destination_end)
            .ok_or_else(|| io::Error::other("preview row lies outside the screen"))?;
        destination_row.copy_from_slice(source_row);
    }
    Ok(())
}

fn write_ppm(path: &Path, pixels: &[u32], dimensions: Dimensions) -> io::Result<()> {
    if pixels.len() != dimensions.pixel_count() {
        return Err(io::Error::other("preview pixel count does not match"));
    }
    let file = File::create(path)?;
    let mut output = BufWriter::new(file);
    write!(
        output,
        "P6\n{} {}\n255\n",
        dimensions.width(),
        dimensions.height()
    )?;
    let row_bytes = dimensions
        .width()
        .checked_mul(3)
        .ok_or_else(|| io::Error::other("preview row size overflowed"))?;
    let mut encoded = vec![0_u8; row_bytes];
    for row in pixels.chunks_exact(dimensions.width()) {
        for (output_pixel, pixel) in encoded.chunks_exact_mut(3).zip(row) {
            let [_, red, green, blue] = pixel.to_be_bytes();
            output_pixel.copy_from_slice(&[red, green, blue]);
        }
        output.write_all(&encoded)?;
    }
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centering_leaves_an_equal_black_safe_edge() {
        let Some(source_dimensions) = Dimensions::new(2, 2) else {
            return;
        };
        let Some(destination_dimensions) = Dimensions::new(4, 4) else {
            return;
        };
        let source = [1, 2, 3, 4];
        let mut destination = [BLACK; 16];
        assert!(
            copy_centered(
                &source,
                source_dimensions,
                &mut destination,
                destination_dimensions
            )
            .is_ok()
        );
        assert_eq!(destination.get(5..7), Some([1, 2].as_slice()));
        assert_eq!(destination.get(9..11), Some([3, 4].as_slice()));
        assert_eq!(destination.first(), Some(&BLACK));
        assert_eq!(destination.last(), Some(&BLACK));
    }
}
