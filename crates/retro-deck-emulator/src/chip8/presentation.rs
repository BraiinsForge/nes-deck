//! Fixed CHIP-8 presentation geometry with crisp nearest-neighbor expansion.

use super::CoreFrame;

/// Stable source width presented for both CHIP-8 and SCHIP modes.
pub const NORMALIZED_FRAME_WIDTH: usize = 128;
/// Stable source height presented for both CHIP-8 and SCHIP modes.
pub const NORMALIZED_FRAME_HEIGHT: usize = 64;
/// Complete normalized indexed framebuffer length.
pub const NORMALIZED_FRAME_PIXELS: usize = NORMALIZED_FRAME_WIDTH * NORMALIZED_FRAME_HEIGHT;

/// Persistent 128x64 frame that absorbs low-to-high resolution switches.
///
/// The compositor therefore keeps one fixed integer scale plan. Ordinary
/// 64x32 CHIP-8 pixels expand to exact 2x2 blocks before that second integer
/// scale, while 128x64 SCHIP pixels remain unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedFrame {
    pixels: [u8; NORMALIZED_FRAME_PIXELS],
    palette: [u32; 4],
}

impl NormalizedFrame {
    /// Construct an empty black frame.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pixels: [0; NORMALIZED_FRAME_PIXELS],
            palette: [0; 4],
        }
    }

    /// Replace the complete frame and palette without allocation.
    pub fn update(&mut self, source: CoreFrame<'_>) {
        let horizontal_scale = NORMALIZED_FRAME_WIDTH / source.width();
        let vertical_scale = NORMALIZED_FRAME_HEIGHT / source.height();
        for (target_y, row) in self
            .pixels
            .chunks_exact_mut(NORMALIZED_FRAME_WIDTH)
            .enumerate()
        {
            let source_y = target_y / vertical_scale;
            for (target_x, target) in row.iter_mut().enumerate() {
                let source_x = target_x / horizontal_scale;
                let source_index = source_y
                    .checked_mul(source.width())
                    .and_then(|offset| offset.checked_add(source_x));
                *target = source_index
                    .and_then(|index| source.pixels().get(index))
                    .copied()
                    .unwrap_or(0);
            }
        }
        self.palette = *source.palette();
    }

    /// Complete row-major 128x64 palette indexes.
    #[must_use]
    pub const fn pixels(&self) -> &[u8; NORMALIZED_FRAME_PIXELS] {
        &self.pixels
    }

    /// Four RGB colors copied from the latest core frame.
    #[must_use]
    pub const fn palette(&self) -> &[u32; 4] {
        &self.palette
    }
}

impl Default for NormalizedFrame {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chip8::{Core, CoreOptions, Quirks};

    #[test]
    fn low_resolution_pixels_become_exact_two_by_two_blocks() {
        let rom = [
            0x00, 0xe0, 0x60, 0x00, 0x61, 0x00, 0xa0, 0x00, 0xd0, 0x15, 0x00, 0xfd,
        ];
        let mut core = Core::new(&rom, CoreOptions::default()).expect("valid core");
        let _ = core.run_frame();
        let source = core.frame().expect("valid low-resolution frame");
        let mut normalized = NormalizedFrame::new();
        normalized.update(source);

        for (source_y, source_row) in source.pixels().chunks_exact(64).enumerate() {
            for (source_x, expected) in source_row.iter().copied().enumerate() {
                for target_y in [source_y * 2, source_y * 2 + 1] {
                    for target_x in [source_x * 2, source_x * 2 + 1] {
                        let index = target_y * NORMALIZED_FRAME_WIDTH + target_x;
                        assert_eq!(normalized.pixels().get(index), Some(&expected));
                    }
                }
            }
        }
        assert_eq!(normalized.palette(), source.palette());
    }

    #[test]
    fn high_resolution_pixels_and_palette_are_preserved() {
        let options = CoreOptions::new(1, Quirks::default(), [1, 2, 3, 4])
            .expect("valid one-instruction options");
        let mut core = Core::new(&[0x00, 0xff], options).expect("valid core");
        let _ = core.run_frame();
        let source = core.frame().expect("valid high-resolution frame");
        let mut normalized = NormalizedFrame::new();
        normalized.update(source);

        assert_eq!(normalized.pixels().as_slice(), source.pixels());
        assert_eq!(normalized.palette(), source.palette());
    }
}
