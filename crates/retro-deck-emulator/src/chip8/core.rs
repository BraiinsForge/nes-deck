use std::borrow::Cow;
use std::error::Error;
use std::ffi::{CStr, c_char};
use std::fmt;
use std::ptr::NonNull;
use std::slice;

use super::{CoreOptions, KeypadState};

/// Maximum program size after the CHIP-8 interpreter area.
pub const MAXIMUM_ROM_BYTES: usize = 65_024;

#[repr(C)]
struct NativeOptions {
    instructions_per_frame: u32,
    quirks: u8,
    reserved: [u8; 3],
}

#[repr(C)]
struct NativeCore {
    private: [u8; 0],
}

unsafe extern "C" {
    fn rd_c_octo_create(
        rom: *const u8,
        size: usize,
        options: *const NativeOptions,
    ) -> *mut NativeCore;
    fn rd_c_octo_destroy(core: *mut NativeCore);
    fn rd_c_octo_set_keys(core: *mut NativeCore, keys: u16);
    fn rd_c_octo_run_frame(core: *mut NativeCore) -> i32;
    fn rd_c_octo_pixels(core: *const NativeCore, width: *mut u32, height: *mut u32) -> *const u8;
    fn rd_c_octo_halted(core: *const NativeCore) -> i32;
    fn rd_c_octo_halt_message(core: *const NativeCore) -> *const c_char;
}

/// Owned c-octo instance behind a validated narrow native boundary.
pub struct Core {
    native: NonNull<NativeCore>,
    palette: [u32; 4],
}

impl Core {
    /// Initialize c-octo with one complete ROM and validated options.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError`] when the ROM is empty, exceeds the interpreter's
    /// address space, or native allocation fails.
    pub fn new(rom: &[u8], options: CoreOptions) -> Result<Self, CoreError> {
        if rom.is_empty() {
            return Err(CoreError::EmptyRom);
        }
        if rom.len() > MAXIMUM_ROM_BYTES {
            return Err(CoreError::RomTooLarge { bytes: rom.len() });
        }
        let native_options = NativeOptions {
            instructions_per_frame: options.instructions_per_frame(),
            quirks: options.quirks().bits(),
            reserved: [0; 3],
        };
        // SAFETY: the ROM and options are valid for the complete synchronous
        // initialization call. The adapter copies ROM bytes into its owned
        // emulator allocation and retains no borrowed pointer.
        let native =
            unsafe { rd_c_octo_create(rom.as_ptr(), rom.len(), &raw const native_options) };
        let native = NonNull::new(native).ok_or(CoreError::InitializationFailed)?;
        Ok(Self {
            native,
            palette: options.palette(),
        })
    }

    /// Replace the complete emulated keypad state before a frame step.
    pub fn set_keypad(&mut self, keypad: KeypadState) {
        // SAFETY: `native` remains uniquely owned and live until `Drop`.
        unsafe { rd_c_octo_set_keys(self.native.as_ptr(), keypad.bits()) };
    }

    /// Execute one bounded 60 Hz instruction and timer step.
    #[must_use]
    pub fn run_frame(&mut self) -> FrameOutcome {
        // SAFETY: `native` remains uniquely owned and live until `Drop`.
        let sound_active = unsafe { rd_c_octo_run_frame(self.native.as_ptr()) } != 0;
        FrameOutcome { sound_active }
    }

    /// Borrow the current low-resolution or high-resolution indexed frame.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError`] if the native adapter violates its fixed frame
    /// contract. No unchecked native dimensions reach a display backend.
    pub fn frame(&self) -> Result<CoreFrame<'_>, FrameError> {
        let mut width = 0_u32;
        let mut height = 0_u32;
        // SAFETY: output pointers are valid for the synchronous call and the
        // returned storage belongs to the live, immutably borrowed core.
        let pixels =
            unsafe { rd_c_octo_pixels(self.native.as_ptr(), &raw mut width, &raw mut height) };
        if pixels.is_null() {
            return Err(FrameError::MissingPixels);
        }
        let (width, height) = match (width, height) {
            (64, 32) => (64_usize, 32_usize),
            (128, 64) => (128_usize, 64_usize),
            _ => return Err(FrameError::InvalidDimensions { width, height }),
        };
        let length = width
            .checked_mul(height)
            .ok_or(FrameError::InvalidDimensions {
                width: u32::try_from(width).unwrap_or(u32::MAX),
                height: u32::try_from(height).unwrap_or(u32::MAX),
            })?;
        // SAFETY: the adapter guarantees an 8192-byte pixel array for the
        // lifetime of the core. Both accepted geometries are no larger than
        // that array, and `self` keeps the allocation immutably borrowed.
        let pixels = unsafe { slice::from_raw_parts(pixels, length) };
        Ok(CoreFrame {
            width,
            height,
            pixels,
            palette: &self.palette,
        })
    }

    /// Return whether c-octo stopped executing instructions.
    #[must_use]
    pub fn halted(&self) -> bool {
        // SAFETY: `native` is live and immutable for this query.
        unsafe { rd_c_octo_halted(self.native.as_ptr()) != 0 }
    }

    /// Borrow a nonempty native diagnostic after an abnormal halt.
    #[must_use]
    pub fn halt_message(&self) -> Option<Cow<'_, str>> {
        // SAFETY: the adapter returns the live core's fixed NUL-terminated
        // message array. It remains valid until the next mutable call or drop.
        let message = unsafe { CStr::from_ptr(rd_c_octo_halt_message(self.native.as_ptr())) };
        if message.is_empty() {
            None
        } else {
            Some(message.to_string_lossy())
        }
    }
}

impl fmt::Debug for Core {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Core")
            .field("native", &self.native)
            .field("palette", &self.palette)
            .field("halted", &self.halted())
            .finish()
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // SAFETY: this is the one matching destruction of the uniquely owned
        // allocation, and `native` is never used afterward.
        unsafe { rd_c_octo_destroy(self.native.as_ptr()) };
    }
}

/// Result of one 60 Hz emulator step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameOutcome {
    sound_active: bool,
}

impl FrameOutcome {
    /// Whether the CHIP-8 sound timer was active for this frame.
    #[must_use]
    pub const fn sound_active(self) -> bool {
        self.sound_active
    }
}

/// Borrowed indexed framebuffer with an owned-core lifetime.
#[derive(Clone, Copy, Debug)]
pub struct CoreFrame<'core> {
    width: usize,
    height: usize,
    pixels: &'core [u8],
    palette: &'core [u32; 4],
}

impl<'core> CoreFrame<'core> {
    /// Visible pixel width and row stride.
    #[must_use]
    pub const fn width(self) -> usize {
        self.width
    }

    /// Visible pixel height.
    #[must_use]
    pub const fn height(self) -> usize {
        self.height
    }

    /// Row-major palette indexes with a stride equal to [`Self::width`].
    #[must_use]
    pub const fn pixels(self) -> &'core [u8] {
        self.pixels
    }

    /// Four 24-bit RGB entries for indexes zero through three.
    #[must_use]
    pub const fn palette(self) -> &'core [u32; 4] {
        self.palette
    }
}

/// ROM validation or native c-octo initialization failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreError {
    /// ROM contains no program bytes.
    EmptyRom,
    /// ROM exceeds available memory after address `0x200`.
    RomTooLarge {
        /// Actual byte count.
        bytes: usize,
    },
    /// The native adapter rejected validated inputs or could not allocate.
    InitializationFailed,
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRom => formatter.write_str("CHIP-8 ROM is empty"),
            Self::RomTooLarge { bytes } => write!(
                formatter,
                "CHIP-8 ROM contains {bytes} bytes; maximum is {MAXIMUM_ROM_BYTES}"
            ),
            Self::InitializationFailed => formatter.write_str("cannot initialize the c-octo core"),
        }
    }
}

impl Error for CoreError {}

/// Native framebuffer contract violation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    /// The native adapter returned no pixel storage.
    MissingPixels,
    /// The native adapter returned neither CHIP-8 nor SCHIP geometry.
    InvalidDimensions {
        /// Reported pixel width.
        width: u32,
        /// Reported pixel height.
        height: u32,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPixels => formatter.write_str("c-octo returned no framebuffer"),
            Self::InvalidDimensions { width, height } => {
                write!(
                    formatter,
                    "c-octo returned invalid frame geometry {width}x{height}"
                )
            }
        }
    }
}

impl Error for FrameError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chip8::{Controller, ControllerButton, ControllerState, InputProfile, Quirks};

    fn one_instruction_options() -> CoreOptions {
        CoreOptions::new(1, Quirks::default(), [0, 1, 2, 3])
            .expect("one instruction per frame is valid")
    }

    #[test]
    fn rom_bounds_are_enforced_before_ffi() {
        assert_eq!(
            Core::new(&[], CoreOptions::default()).err(),
            Some(CoreError::EmptyRom)
        );
        assert_eq!(
            Core::new(&vec![0; MAXIMUM_ROM_BYTES + 1], CoreOptions::default()).err(),
            Some(CoreError::RomTooLarge {
                bytes: MAXIMUM_ROM_BYTES + 1
            })
        );
    }

    #[test]
    fn core_draws_the_builtin_font_and_exits_cleanly() {
        let rom = [
            0x00, 0xe0, 0x60, 0x00, 0x61, 0x00, 0xa0, 0x00, 0xd0, 0x15, 0x00, 0xfd,
        ];
        let mut core = Core::new(&rom, CoreOptions::default()).expect("valid core");
        assert!(!core.run_frame().sound_active());
        assert!(core.halted());
        assert_eq!(core.halt_message(), None);
        let frame = core.frame().expect("valid low-resolution frame");
        assert_eq!((frame.width(), frame.height()), (64, 32));
        assert!(frame.pixels().iter().any(|pixel| *pixel != 0));
        assert_eq!(
            frame.palette(),
            &[0x00_00_00, 0xff_cc_00, 0xff_66_00, 0x66_22_00]
        );
    }

    #[test]
    fn schip_mode_exposes_the_complete_high_resolution_frame() {
        let mut core = Core::new(&[0x00, 0xff], one_instruction_options()).expect("valid core");
        let _ = core.run_frame();
        let frame = core.frame().expect("valid SCHIP frame");
        assert_eq!((frame.width(), frame.height()), (128, 64));
        assert_eq!(frame.pixels().len(), 128 * 64);
    }

    #[test]
    fn blocking_key_input_completes_on_release_like_c_octo() {
        let options = CoreOptions::new(4, Quirks::default(), [0, 1, 2, 3])
            .expect("four instructions per frame is valid");
        let mut core = Core::new(&[0xf0, 0x0a, 0x00, 0xfd], options).expect("valid core");
        let mut controllers = ControllerState::default();
        controllers.set(Controller::One, ControllerButton::B, true);
        core.set_keypad(controllers.keypad(InputProfile::Octo));
        let _ = core.run_frame();
        assert!(!core.halted());

        controllers.set(Controller::One, ControllerButton::B, false);
        core.set_keypad(controllers.keypad(InputProfile::Octo));
        let _ = core.run_frame();
        assert!(core.halted());
    }

    #[test]
    fn abnormal_halts_preserve_the_upstream_diagnostic() {
        let mut core = Core::new(&[0x80, 0x08], one_instruction_options()).expect("valid core");
        let _ = core.run_frame();
        assert!(core.halted());
        assert_eq!(
            core.halt_message().as_deref(),
            Some("Unknown Math Opcode 0x8008")
        );
    }
}
