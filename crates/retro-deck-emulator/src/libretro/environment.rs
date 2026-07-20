//! Audited environment callback state around the raw libretro ABI.

#![allow(
    dead_code,
    reason = "the environment object is wired into the executable in the next host migration slice"
)]

use std::error::Error;
use std::ffi::{CStr, CString, c_uint, c_void};
use std::fmt;
use std::path::{Path, PathBuf};
use std::ptr;

use super::{LibretroCore, abi};

/// Validated software pixel formats supported by the Deck presentation path.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PixelFormat {
    /// Native-endian XRGB8888 with the high byte ignored.
    #[default]
    Xrgb8888,
    /// Native-endian RGB565.
    Rgb565,
}

/// Stable strings and negotiated state exposed to one statically linked core.
#[derive(Debug)]
pub(super) struct Environment {
    core: LibretroCore,
    directory: CString,
    option_values: Vec<CString>,
    pixel_format: PixelFormat,
}

impl Environment {
    /// Build stable C strings for one loaded content directory and core.
    pub(super) fn new(core: LibretroCore, directory: &Path) -> Result<Self, EnvironmentError> {
        let Some(directory_text) = directory.to_str() else {
            return Err(EnvironmentError::new(
                directory,
                EnvironmentFailure::NotUtf8,
            ));
        };
        let directory = CString::new(directory_text)
            .map_err(|_| EnvironmentError::new(directory, EnvironmentFailure::InteriorNul))?;
        let option_values = core
            .options()
            .iter()
            .map(|option| CString::new(option.value()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                EnvironmentError::new(Path::new(core.core_name()), EnvironmentFailure::InteriorNul)
            })?;
        Ok(Self {
            core,
            directory,
            option_values,
            pixel_format: PixelFormat::default(),
        })
    }

    pub(super) const fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    /// Dispatch one raw callback from the trusted statically linked core.
    ///
    /// # Safety
    ///
    /// For commands with a data parameter, `data` must be null or point to
    /// the exact writable C type specified by libretro API version 1. Any C
    /// strings reachable through that type must be valid and NUL-terminated.
    pub(super) unsafe fn dispatch(&mut self, command: c_uint, data: *mut c_void) -> bool {
        match command {
            abi::ENVIRONMENT_GET_CORE_OPTIONS_VERSION => {
                // SAFETY: The caller upholds the command-specific pointer contract.
                unsafe { write_output(data, 2_u32) }
            }
            abi::ENVIRONMENT_GET_LANGUAGE => {
                // SAFETY: The caller upholds the command-specific pointer contract.
                unsafe { write_output(data, abi::LANGUAGE_ENGLISH) }
            }
            abi::ENVIRONMENT_GET_SYSTEM_DIRECTORY
            | abi::ENVIRONMENT_GET_CONTENT_DIRECTORY
            | abi::ENVIRONMENT_GET_SAVE_DIRECTORY => {
                // SAFETY: The caller upholds the command-specific pointer contract.
                unsafe { write_output(data, self.directory.as_ptr()) }
            }
            abi::ENVIRONMENT_GET_CAN_DUPE => {
                // SAFETY: The caller upholds the command-specific pointer contract.
                unsafe { write_output(data, true) }
            }
            abi::ENVIRONMENT_GET_VARIABLE_UPDATE => {
                // SAFETY: The caller upholds the command-specific pointer contract.
                unsafe { write_output(data, false) }
            }
            abi::ENVIRONMENT_GET_VARIABLE => {
                // SAFETY: The caller upholds the command-specific pointer contract.
                unsafe { self.get_variable(data) }
            }
            abi::ENVIRONMENT_SET_PIXEL_FORMAT => {
                // SAFETY: The caller upholds the command-specific pointer contract.
                let Some(format) = (unsafe { read_input::<c_uint>(data) }) else {
                    return false;
                };
                match format {
                    abi::PIXEL_FORMAT_XRGB8888 => self.pixel_format = PixelFormat::Xrgb8888,
                    abi::PIXEL_FORMAT_RGB565 => self.pixel_format = PixelFormat::Rgb565,
                    _ => return false,
                }
                true
            }
            abi::ENVIRONMENT_GET_INPUT_BITMASKS
            | abi::ENVIRONMENT_SET_PERFORMANCE_LEVEL
            | abi::ENVIRONMENT_SET_INPUT_DESCRIPTORS
            | abi::ENVIRONMENT_SET_VARIABLES
            | abi::ENVIRONMENT_SET_SYSTEM_AV_INFO
            | abi::ENVIRONMENT_SET_SUBSYSTEM_INFO
            | abi::ENVIRONMENT_SET_CONTROLLER_INFO
            | abi::ENVIRONMENT_SET_MEMORY_MAPS
            | abi::ENVIRONMENT_SET_GEOMETRY
            | abi::ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS
            | abi::ENVIRONMENT_SET_CORE_OPTIONS
            | abi::ENVIRONMENT_SET_CORE_OPTIONS_INTL
            | abi::ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY
            | abi::ENVIRONMENT_SET_CORE_OPTIONS_V2
            | abi::ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL => true,
            _ => false,
        }
    }

    unsafe fn get_variable(&self, data: *mut c_void) -> bool {
        // SAFETY: The caller guarantees a writable `Variable` for this command.
        let Some(variable) = (unsafe { data.cast::<abi::Variable>().as_mut() }) else {
            return false;
        };
        variable.value = ptr::null();
        if variable.key.is_null() {
            return false;
        }
        // SAFETY: The dispatch contract requires a valid NUL-terminated key.
        let key = unsafe { CStr::from_ptr(variable.key) }.to_bytes();
        let value = self
            .core
            .options()
            .iter()
            .zip(&self.option_values)
            .find(|(option, _)| option.key().as_bytes() == key)
            .map(|(_, value)| value.as_ptr());
        let Some(value) = value else {
            return false;
        };
        variable.value = value;
        true
    }
}

/// Invalid path data while preparing stable libretro strings.
#[derive(Debug)]
pub(super) struct EnvironmentError {
    path: PathBuf,
    failure: EnvironmentFailure,
}

impl EnvironmentError {
    fn new(path: &Path, failure: EnvironmentFailure) -> Self {
        Self {
            path: path.to_owned(),
            failure,
        }
    }
}

impl fmt::Display for EnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.failure {
            EnvironmentFailure::NotUtf8 => write!(
                formatter,
                "libretro directory is not valid UTF-8: {}",
                self.path.display()
            ),
            EnvironmentFailure::InteriorNul => write!(
                formatter,
                "libretro string contains an interior NUL: {}",
                self.path.display()
            ),
        }
    }
}

impl Error for EnvironmentError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EnvironmentFailure {
    NotUtf8,
    InteriorNul,
}

unsafe fn write_output<T>(data: *mut c_void, value: T) -> bool {
    // SAFETY: The caller guarantees a writable `T` for the selected command.
    let Some(output) = (unsafe { data.cast::<T>().as_mut() }) else {
        return false;
    };
    *output = value;
    true
}

const unsafe fn read_input<T: Copy>(data: *mut c_void) -> Option<T> {
    // SAFETY: The caller guarantees a readable `T` for the selected command.
    unsafe { data.cast::<T>().as_ref() }.copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsString, c_char};
    use std::os::unix::ffi::OsStringExt as _;

    #[test]
    fn scalar_and_directory_queries_are_exact() {
        let mut environment =
            Environment::new(LibretroCore::Fceumm, Path::new("/mnt/data/roms/nes"))
                .expect("ASCII directory");
        let mut version = 0_u32;
        // SAFETY: `version` has the type required by this command.
        assert!(unsafe {
            environment.dispatch(
                abi::ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
                ptr::from_mut(&mut version).cast(),
            )
        });
        assert_eq!(version, 2);

        let mut language = u32::MAX;
        // SAFETY: `language` has the type required by this command.
        assert!(unsafe {
            environment.dispatch(
                abi::ENVIRONMENT_GET_LANGUAGE,
                ptr::from_mut(&mut language).cast(),
            )
        });
        assert_eq!(language, abi::LANGUAGE_ENGLISH);

        let mut directory: *const c_char = ptr::null();
        for command in [
            abi::ENVIRONMENT_GET_SYSTEM_DIRECTORY,
            abi::ENVIRONMENT_GET_CONTENT_DIRECTORY,
            abi::ENVIRONMENT_GET_SAVE_DIRECTORY,
        ] {
            // SAFETY: `directory` has the type required by each command.
            assert!(unsafe { environment.dispatch(command, ptr::from_mut(&mut directory).cast()) });
            assert!(!directory.is_null());
            // SAFETY: The returned pointer belongs to the live environment.
            assert_eq!(
                unsafe { CStr::from_ptr(directory) }.to_bytes(),
                b"/mnt/data/roms/nes"
            );
        }
        // SAFETY: Null is explicitly accepted and rejected before dereference.
        assert!(!unsafe { environment.dispatch(abi::ENVIRONMENT_GET_LANGUAGE, ptr::null_mut()) });
    }

    #[test]
    fn fixed_options_return_stable_exact_c_strings() {
        let mut environment =
            Environment::new(LibretroCore::Fuse, Path::new("/roms/zx")).expect("ASCII directory");
        let key = CString::new("fuse_machine").expect("static key");
        let mut variable = abi::Variable {
            key: key.as_ptr(),
            value: ptr::null(),
        };
        // SAFETY: `variable` and its key meet the command contract.
        assert!(unsafe {
            environment.dispatch(
                abi::ENVIRONMENT_GET_VARIABLE,
                ptr::from_mut(&mut variable).cast(),
            )
        });
        assert!(!variable.value.is_null());
        // SAFETY: The returned pointer belongs to the live environment.
        assert_eq!(
            unsafe { CStr::from_ptr(variable.value) }.to_bytes(),
            b"Spectrum 48K"
        );

        let unknown = CString::new("fuse_unknown").expect("static key");
        variable.key = unknown.as_ptr();
        variable.value = key.as_ptr();
        // SAFETY: `variable` and its key meet the command contract.
        assert!(!unsafe {
            environment.dispatch(
                abi::ENVIRONMENT_GET_VARIABLE,
                ptr::from_mut(&mut variable).cast(),
            )
        });
        assert!(variable.value.is_null());
    }

    #[test]
    fn pixel_format_negotiation_changes_state_only_on_success() {
        let mut environment = Environment::new(LibretroCore::Gambatte, Path::new("/roms/gb"))
            .expect("ASCII directory");
        assert_eq!(environment.pixel_format(), PixelFormat::Xrgb8888);
        let mut format = abi::PIXEL_FORMAT_RGB565;
        // SAFETY: `format` has the type required by this command.
        assert!(unsafe {
            environment.dispatch(
                abi::ENVIRONMENT_SET_PIXEL_FORMAT,
                ptr::from_mut(&mut format).cast(),
            )
        });
        assert_eq!(environment.pixel_format(), PixelFormat::Rgb565);
        format = 0;
        // SAFETY: `format` has the type required by this command.
        assert!(!unsafe {
            environment.dispatch(
                abi::ENVIRONMENT_SET_PIXEL_FORMAT,
                ptr::from_mut(&mut format).cast(),
            )
        });
        assert_eq!(environment.pixel_format(), PixelFormat::Rgb565);
    }

    #[test]
    fn supported_notifications_and_capabilities_are_explicit() {
        let mut environment =
            Environment::new(LibretroCore::Fceumm, Path::new(".")).expect("ASCII directory");
        for command in [
            abi::ENVIRONMENT_GET_INPUT_BITMASKS,
            abi::ENVIRONMENT_SET_PERFORMANCE_LEVEL,
            abi::ENVIRONMENT_SET_INPUT_DESCRIPTORS,
            abi::ENVIRONMENT_SET_VARIABLES,
            abi::ENVIRONMENT_SET_CONTROLLER_INFO,
            abi::ENVIRONMENT_SET_MEMORY_MAPS,
            abi::ENVIRONMENT_SET_GEOMETRY,
            abi::ENVIRONMENT_SET_CORE_OPTIONS_V2,
        ] {
            // SAFETY: These accepted notifications do not dereference data.
            assert!(unsafe { environment.dispatch(command, ptr::null_mut()) });
        }
        // SAFETY: These unsupported commands do not dereference data.
        assert!(!unsafe {
            environment.dispatch(abi::ENVIRONMENT_GET_LOG_INTERFACE, ptr::null_mut())
        });
        assert!(!unsafe { environment.dispatch(c_uint::MAX, ptr::null_mut()) });
    }

    #[test]
    fn boolean_capabilities_write_complete_values() {
        let mut environment =
            Environment::new(LibretroCore::Fceumm, Path::new(".")).expect("ASCII directory");
        let mut can_duplicate = false;
        // SAFETY: `can_duplicate` has the type required by this command.
        assert!(unsafe {
            environment.dispatch(
                abi::ENVIRONMENT_GET_CAN_DUPE,
                ptr::from_mut(&mut can_duplicate).cast(),
            )
        });
        assert!(can_duplicate);
        let mut updated = true;
        // SAFETY: `updated` has the type required by this command.
        assert!(unsafe {
            environment.dispatch(
                abi::ENVIRONMENT_GET_VARIABLE_UPDATE,
                ptr::from_mut(&mut updated).cast(),
            )
        });
        assert!(!updated);
    }

    #[test]
    fn invalid_directory_strings_fail_before_core_initialization() {
        let non_utf8 = OsString::from_vec(vec![b'/', 0xff]);
        assert!(Environment::new(LibretroCore::Fceumm, Path::new(&non_utf8)).is_err());
        assert!(Environment::new(LibretroCore::Fceumm, Path::new("bad\0path")).is_err());
    }
}
