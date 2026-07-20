//! Exclusive, restorable Linux virtual-console keyboard mode.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags, open};
use rustix::ioctl::{Getter, IntegerSetter, Opcode, ioctl, opcode};
use rustix::termios::{OptionalActions, Termios, tcgetattr, tcsetattr};

use super::KeyboardState;

const CONSOLE_PATHS: [&str; 3] = ["/dev/tty", "/dev/tty0", "/dev/console"];
const KEYBOARD_IOCTL_GROUP: u8 = b'K';
const GET_KEYBOARD_TYPE: Opcode = opcode::none(KEYBOARD_IOCTL_GROUP, 0x33);
const GET_KEYBOARD_MODE: Opcode = opcode::none(KEYBOARD_IOCTL_GROUP, 0x44);
const SET_KEYBOARD_MODE: Opcode = opcode::none(KEYBOARD_IOCTL_GROUP, 0x45);
const KEYBOARD_TYPE_84: u8 = 1;
const KEYBOARD_TYPE_101: u8 = 2;
const KEYBOARD_MODE_MEDIUM_RAW: usize = 2;
const MAXIMUM_KEYBOARD_MODE: usize = 4;
const READ_BYTES: usize = 256;

/// One owned virtual-console keyboard temporarily placed in medium-raw mode.
pub struct MediumRawKeyboard {
    file: File,
    path: PathBuf,
    original_terminal: Termios,
    original_keyboard_mode: usize,
    state: KeyboardState,
    configured: bool,
}

impl MediumRawKeyboard {
    /// Find and configure the first recognized Linux virtual console.
    ///
    /// Every candidate is opened nonblocking and with close-on-exec. A path
    /// that is absent, inaccessible, a symlink, or not a console keyboard is
    /// skipped without mutation.
    ///
    /// # Errors
    ///
    /// Returns [`MediumRawKeyboardError::NotFound`] when no known path is a
    /// usable virtual-console keyboard. Once a console is recognized, any
    /// failure to snapshot or configure it is returned with its exact path and
    /// operation.
    pub fn discover() -> Result<Self, MediumRawKeyboardError> {
        let paths = CONSOLE_PATHS.map(Path::new);
        Self::discover_in(&paths)
    }

    fn discover_in(paths: &[&Path]) -> Result<Self, MediumRawKeyboardError> {
        for path in paths {
            let Ok(descriptor) = open(
                *path,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOFOLLOW,
                Mode::empty(),
            ) else {
                continue;
            };
            let file = File::from(descriptor);
            if !query_keyboard_type(&file).is_ok_and(supported_keyboard_type) {
                continue;
            }
            return Self::configure(file, path);
        }
        Err(MediumRawKeyboardError::NotFound)
    }

    fn configure(file: File, path: &Path) -> Result<Self, MediumRawKeyboardError> {
        let path = path.to_owned();
        let original_terminal = tcgetattr(&file)
            .map_err(|source| operation_error(&path, "terminal snapshot", source.into()))?;
        let original_keyboard_mode = query_keyboard_mode(&file)
            .map_err(|source| operation_error(&path, "keyboard-mode snapshot", source.into()))?;
        let original_keyboard_mode =
            valid_keyboard_mode(original_keyboard_mode).ok_or_else(|| {
                operation_error(
                    &path,
                    "keyboard-mode snapshot",
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "kernel returned an invalid mode",
                    ),
                )
            })?;

        let mut raw_terminal = original_terminal.clone();
        raw_terminal.make_raw();
        let keyboard = Self {
            file,
            path,
            original_terminal,
            original_keyboard_mode,
            state: KeyboardState::empty(),
            configured: true,
        };
        tcsetattr(&keyboard.file, OptionalActions::Flush, &raw_terminal).map_err(|source| {
            operation_error(&keyboard.path, "enable raw terminal", source.into())
        })?;
        set_keyboard_mode(&keyboard.file, KEYBOARD_MODE_MEDIUM_RAW).map_err(|source| {
            operation_error(&keyboard.path, "enable medium-raw keyboard", source.into())
        })?;

        Ok(keyboard)
    }

    /// Configured console path for diagnostics.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "PathBuf-to-Path dereference is not const on the supported Rust toolchain"
    )]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current complete seven-bit key snapshot.
    #[must_use]
    pub const fn state(&self) -> KeyboardState {
        self.state
    }

    /// Consume at most 256 currently available medium-raw bytes without waiting.
    ///
    /// # Errors
    ///
    /// Returns a path-qualified read error when the console disconnects or
    /// reports an error other than interruption or normal `WouldBlock`.
    pub fn drain(&mut self) -> Result<usize, MediumRawKeyboardError> {
        let mut bytes = [0_u8; READ_BYTES];
        let amount = loop {
            match self.file.read(&mut bytes) {
                Ok(0) => {
                    return Err(operation_error(
                        &self.path,
                        "read",
                        io::Error::new(io::ErrorKind::UnexpectedEof, "console disconnected"),
                    ));
                }
                Ok(amount) => break amount,
                Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
                Err(source) if source.kind() == io::ErrorKind::WouldBlock => return Ok(0),
                Err(source) => return Err(operation_error(&self.path, "read", source)),
            }
        };
        apply_bytes(&mut self.state, bytes.get(..amount).unwrap_or_default());
        Ok(amount)
    }

    /// Restore the exact keyboard and terminal modes now instead of on drop.
    ///
    /// Both restoration operations are attempted even when the first fails.
    ///
    /// # Errors
    ///
    /// Returns the first path-qualified restoration failure.
    pub fn restore(mut self) -> Result<(), MediumRawKeyboardError> {
        self.restore_inner()
    }

    fn restore_inner(&mut self) -> Result<(), MediumRawKeyboardError> {
        if !self.configured {
            return Ok(());
        }
        let mode = set_keyboard_mode(&self.file, self.original_keyboard_mode)
            .map_err(|source| operation_error(&self.path, "restore keyboard mode", source.into()));
        let terminal = tcsetattr(&self.file, OptionalActions::Flush, &self.original_terminal)
            .map_err(|source| operation_error(&self.path, "restore terminal", source.into()));
        let result = mode.and(terminal);
        self.configured = result.is_err();
        result
    }
}

impl fmt::Debug for MediumRawKeyboard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MediumRawKeyboard")
            .field("file", &self.file)
            .field("path", &self.path)
            .field("original_keyboard_mode", &self.original_keyboard_mode)
            .field("state", &self.state)
            .field("configured", &self.configured)
            .finish_non_exhaustive()
    }
}

impl AsFd for MediumRawKeyboard {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl Drop for MediumRawKeyboard {
    fn drop(&mut self) {
        let _restored = self.restore_inner();
    }
}

/// Console discovery, configuration, reading, or restoration failure.
#[derive(Debug)]
pub enum MediumRawKeyboardError {
    /// No standard console path referred to a recognized keyboard.
    NotFound,
    /// One operation failed after a console path was recognized.
    Operation {
        /// Console path being accessed.
        path: PathBuf,
        /// Stable operation name.
        operation: &'static str,
        /// Underlying system failure.
        source: io::Error,
    },
}

impl fmt::Display for MediumRawKeyboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("no Linux virtual-console keyboard was found"),
            Self::Operation {
                path,
                operation,
                source,
            } => write!(
                formatter,
                "console {} {operation} failed: {source}",
                path.display()
            ),
        }
    }
}

impl Error for MediumRawKeyboardError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Operation { source, .. } => Some(source),
            Self::NotFound => None,
        }
    }
}

fn operation_error(
    path: &Path,
    operation: &'static str,
    source: io::Error,
) -> MediumRawKeyboardError {
    MediumRawKeyboardError::Operation {
        path: path.to_owned(),
        operation,
        source,
    }
}

const fn supported_keyboard_type(kind: u8) -> bool {
    matches!(kind, KEYBOARD_TYPE_84 | KEYBOARD_TYPE_101)
}

fn valid_keyboard_mode(mode: i32) -> Option<usize> {
    usize::try_from(mode)
        .ok()
        .filter(|mode| *mode <= MAXIMUM_KEYBOARD_MODE)
}

fn apply_bytes(state: &mut KeyboardState, bytes: &[u8]) {
    for byte in bytes {
        state.apply_medium_raw_byte(*byte);
    }
}

fn query_keyboard_type(file: &File) -> rustix::io::Result<u8> {
    // SAFETY: Linux `KDGKBTYPE` writes exactly one byte through its pointer.
    let operation = unsafe { Getter::<GET_KEYBOARD_TYPE, u8>::new() };
    // SAFETY: The operation and output type match Linux `KDGKBTYPE`.
    unsafe { ioctl(file, operation) }
}

fn query_keyboard_mode(file: &File) -> rustix::io::Result<i32> {
    // SAFETY: Linux `KDGKBMODE` writes exactly one C `int` through its pointer.
    let operation = unsafe { Getter::<GET_KEYBOARD_MODE, i32>::new() };
    // SAFETY: The operation and output type match Linux `KDGKBMODE`.
    unsafe { ioctl(file, operation) }
}

fn set_keyboard_mode(file: &File, mode: usize) -> rustix::io::Result<()> {
    // SAFETY: Linux `KDSKBMODE` takes one integer mode. Callers supply only a
    // validated snapshot or the kernel-defined medium-raw constant.
    let operation = unsafe { IntegerSetter::<SET_KEYBOARD_MODE>::new_usize(mode) };
    // SAFETY: The operation and integer operand match Linux `KDSKBMODE`.
    unsafe { ioctl(file, operation) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::MediumRawKey;

    #[test]
    fn legacy_linux_console_opcodes_are_exact() {
        assert_eq!(GET_KEYBOARD_TYPE, 0x4b33);
        assert_eq!(GET_KEYBOARD_MODE, 0x4b44);
        assert_eq!(SET_KEYBOARD_MODE, 0x4b45);
    }

    #[test]
    fn only_linux_console_keyboard_types_are_accepted() {
        assert!(!supported_keyboard_type(0));
        assert!(supported_keyboard_type(KEYBOARD_TYPE_84));
        assert!(supported_keyboard_type(KEYBOARD_TYPE_101));
        assert!(!supported_keyboard_type(3));
    }

    #[test]
    fn only_defined_keyboard_modes_can_be_restored() {
        assert_eq!(valid_keyboard_mode(-1), None);
        assert_eq!(valid_keyboard_mode(0), Some(0));
        assert_eq!(valid_keyboard_mode(4), Some(4));
        assert_eq!(valid_keyboard_mode(5), None);
    }

    #[test]
    fn medium_raw_bytes_update_a_complete_snapshot() {
        let letter_a = MediumRawKey::new(30).expect("A key code");
        let arrow_up = MediumRawKey::new(103).expect("up key code");
        let mut state = KeyboardState::empty();
        apply_bytes(&mut state, &[30, 103]);
        assert!(state.contains(letter_a));
        assert!(state.contains(arrow_up));
        apply_bytes(&mut state, &[30 | 0x80]);
        assert!(!state.contains(letter_a));
        assert!(state.contains(arrow_up));
    }

    #[test]
    fn ordinary_files_are_never_configured_as_keyboards() {
        let path = Path::new("/dev/null");
        assert!(matches!(
            MediumRawKeyboard::discover_in(&[path]),
            Err(MediumRawKeyboardError::NotFound)
        ));
    }
}
