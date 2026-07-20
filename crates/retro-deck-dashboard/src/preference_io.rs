//! Optional preference loading with independent safe fallbacks.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use retro_deck_platform::file::{BoundedReadError, read_regular_bounded};

use crate::{
    DashboardPreferences, MAXIMUM_PREFERENCE_BYTES, PreferenceField, PreferenceValueError,
    parse_brightness, parse_keymap, parse_volume,
};

/// Absolute locations of the three small persistent dashboard values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreferencePaths {
    volume: PathBuf,
    brightness: PathBuf,
    keymap: PathBuf,
}

impl PreferencePaths {
    /// Validate distinct absolute paths without touching the filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`PreferencePathError`] for a relative or repeated path.
    pub fn new(
        volume: impl Into<PathBuf>,
        brightness: impl Into<PathBuf>,
        keymap: impl Into<PathBuf>,
    ) -> Result<Self, PreferencePathError> {
        let paths = Self {
            volume: volume.into(),
            brightness: brightness.into(),
            keymap: keymap.into(),
        };
        for (field, path) in [
            (PreferenceField::Volume, paths.volume.as_path()),
            (PreferenceField::Brightness, paths.brightness.as_path()),
            (PreferenceField::Keymap, paths.keymap.as_path()),
        ] {
            if !path.is_absolute() {
                return Err(PreferencePathError::Relative {
                    field,
                    path: path.to_path_buf(),
                });
            }
        }
        if paths.volume == paths.brightness
            || paths.volume == paths.keymap
            || paths.brightness == paths.keymap
        {
            return Err(PreferencePathError::Duplicate);
        }
        Ok(paths)
    }

    /// Volume state destination.
    #[must_use]
    pub fn volume(&self) -> &Path {
        self.volume.as_path()
    }

    /// Brightness state destination.
    #[must_use]
    pub fn brightness(&self) -> &Path {
        self.brightness.as_path()
    }

    /// Terminal keymap state destination.
    #[must_use]
    pub fn keymap(&self) -> &Path {
        self.keymap.as_path()
    }
}

/// Startup values plus at most one fallback diagnostic per field.
#[derive(Debug)]
pub struct PreferenceLoad {
    preferences: DashboardPreferences,
    issues: Vec<PreferenceLoadIssue>,
}

impl PreferenceLoad {
    /// Load each preference independently through a bounded regular descriptor.
    #[must_use]
    pub fn load(paths: &PreferencePaths) -> Self {
        let defaults = DashboardPreferences::default();
        let mut issues = Vec::with_capacity(3);

        let volume = match read_value(paths.volume(), parse_volume) {
            Ok(value) => value,
            Err(error) => {
                issues.push(PreferenceLoadIssue {
                    field: PreferenceField::Volume,
                    error,
                });
                defaults.volume()
            }
        };
        let brightness = match read_value(paths.brightness(), parse_brightness) {
            Ok(value) => value,
            Err(error) => {
                issues.push(PreferenceLoadIssue {
                    field: PreferenceField::Brightness,
                    error,
                });
                defaults.brightness()
            }
        };
        let keymap = match read_value(paths.keymap(), parse_keymap) {
            Ok(value) => value,
            Err(error) => {
                issues.push(PreferenceLoadIssue {
                    field: PreferenceField::Keymap,
                    error,
                });
                defaults.keymap()
            }
        };

        Self {
            preferences: DashboardPreferences::new(volume, brightness, keymap),
            issues,
        }
    }

    /// Valid values after per-field fallback.
    #[must_use]
    pub const fn preferences(&self) -> DashboardPreferences {
        self.preferences
    }

    /// Ordered volume, brightness, and keymap fallback diagnostics.
    #[must_use]
    pub fn issues(&self) -> impl ExactSizeIterator<Item = &PreferenceLoadIssue> {
        self.issues.iter()
    }
}

fn read_value<T>(
    path: &Path,
    parse: fn(&[u8]) -> Result<T, PreferenceValueError>,
) -> Result<T, PreferenceLoadError> {
    let bytes =
        read_regular_bounded(path, MAXIMUM_PREFERENCE_BYTES).map_err(PreferenceLoadError::Read)?;
    parse(&bytes).map_err(PreferenceLoadError::Value)
}

/// One preference path violates the startup contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreferencePathError {
    /// One destination is not absolute.
    Relative {
        /// Field assigned to this path.
        field: PreferenceField,
        /// Rejected destination.
        path: PathBuf,
    },
    /// Two fields would overwrite the same destination.
    Duplicate,
}

impl fmt::Display for PreferencePathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relative { field, path } => {
                write!(
                    formatter,
                    "{field} preference path is not absolute: {}",
                    path.display()
                )
            }
            Self::Duplicate => formatter.write_str("preference paths must be distinct"),
        }
    }
}

impl Error for PreferencePathError {}

impl fmt::Display for PreferenceField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Volume => "volume",
            Self::Brightness => "brightness",
            Self::Keymap => "keymap",
        })
    }
}

/// One field used its compiled fallback.
#[derive(Debug)]
pub struct PreferenceLoadIssue {
    field: PreferenceField,
    error: PreferenceLoadError,
}

impl PreferenceLoadIssue {
    /// Field whose state was unavailable.
    #[must_use]
    pub const fn field(&self) -> PreferenceField {
        self.field
    }

    /// File or schema failure that selected the fallback.
    #[must_use]
    pub const fn error(&self) -> &PreferenceLoadError {
        &self.error
    }
}

impl fmt::Display for PreferenceLoadIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot load {} preference: {}; using compiled default",
            self.field, self.error
        )
    }
}

impl Error for PreferenceLoadIssue {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

/// Bounded read or exact-value failure for one preference.
#[derive(Debug)]
pub enum PreferenceLoadError {
    /// The state file was missing, unsafe, unreadable, empty, or oversized.
    Read(BoundedReadError),
    /// The complete bytes did not match the field schema.
    Value(PreferenceValueError),
}

impl fmt::Display for PreferenceLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => error.fmt(formatter),
            Self::Value(error) => error.fmt(formatter),
        }
    }
}

impl Error for PreferenceLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Value(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{PreferenceLoad, PreferenceLoadIssue, PreferencePathError, PreferencePaths};
    use crate::{Keymap, PreferenceField};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "retro-deck-preferences-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("preference fixture directory is created");
            Self(path)
        }

        fn paths(&self) -> PreferencePaths {
            PreferencePaths::new(
                self.0.join("volume"),
                self.0.join("brightness"),
                self.0.join("keymap"),
            )
            .expect("fixture preference paths are valid")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn one_bad_field_does_not_discard_two_valid_fields() {
        let fixture = Fixture::new();
        let paths = fixture.paths();
        fs::write(paths.volume(), b"0\n").expect("volume state is written");
        fs::write(paths.brightness(), b"65\n").expect("brightness state is written");
        fs::write(paths.keymap(), b"cz\n").expect("keymap state is written");

        let loaded = PreferenceLoad::load(&paths);
        let preferences = loaded.preferences();
        assert!(preferences.volume().is_muted());
        assert_eq!(preferences.brightness().percent(), 60);
        assert_eq!(preferences.keymap(), Keymap::Czech);
        assert_eq!(loaded.issues().len(), 1);
        assert_eq!(
            loaded.issues().next().map(PreferenceLoadIssue::field),
            Some(PreferenceField::Brightness)
        );
    }

    #[test]
    fn missing_and_symlink_state_select_only_safe_defaults() {
        let fixture = Fixture::new();
        let paths = fixture.paths();
        fs::write(fixture.0.join("target"), b"100\n").expect("symlink target is written");
        symlink(fixture.0.join("target"), paths.volume()).expect("state symlink is created");

        let loaded = PreferenceLoad::load(&paths);
        assert_eq!(loaded.preferences(), crate::DashboardPreferences::default());
        assert_eq!(loaded.issues().len(), 3);
    }

    #[test]
    fn paths_must_be_absolute_and_distinct() {
        assert!(matches!(
            PreferencePaths::new("relative", "/state/brightness", "/state/keymap"),
            Err(PreferencePathError::Relative {
                field: PreferenceField::Volume,
                ..
            })
        ));
        assert_eq!(
            PreferencePaths::new("/state/value", "/state/value", "/state/keymap"),
            Err(PreferencePathError::Duplicate)
        );
    }
}
