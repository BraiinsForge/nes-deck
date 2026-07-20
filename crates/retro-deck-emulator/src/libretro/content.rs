//! Bounded no-follow loading for libretro content images.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags, open};

use super::{LibretroCore, MAXIMUM_ROM_BYTES};

/// One bounded content image ready to pass to a statically linked core.
pub struct Content {
    core: LibretroCore,
    path: PathBuf,
    bytes: Vec<u8>,
}

impl Content {
    /// Open and read one regular content image for `core`.
    ///
    /// The filename extension must be canonical lowercase. The final path
    /// component cannot be a symlink. Data is read through the already-open
    /// descriptor and bounded one byte beyond the global limit so concurrent
    /// size changes cannot silently truncate the image.
    ///
    /// # Errors
    ///
    /// Returns [`ContentError`] for an extension, filesystem type, size,
    /// concurrent-change, open, or read failure.
    pub fn load(core: LibretroCore, path: &Path) -> Result<Self, ContentError> {
        let extension = path.extension().and_then(|value| value.to_str());
        if !extension.is_some_and(|value| core.extensions().contains(&value)) {
            return Err(ContentError::new(
                path,
                ContentFailure::WrongExtension { core },
            ));
        }

        let descriptor = open(
            path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|source| {
            ContentError::new(
                path,
                ContentFailure::Open {
                    source: io::Error::from(source),
                },
            )
        })?;
        let mut file = File::from(descriptor);
        let metadata = file
            .metadata()
            .map_err(|source| ContentError::new(path, ContentFailure::Read { source }))?;
        if !metadata.is_file() {
            return Err(ContentError::new(path, ContentFailure::NotRegular));
        }
        let expected = usize::try_from(metadata.len()).map_err(|_| {
            ContentError::new(
                path,
                ContentFailure::InvalidSize {
                    bytes: usize::MAX,
                    minimum: core.minimum_rom_bytes(),
                },
            )
        })?;
        if expected < core.minimum_rom_bytes() || expected > MAXIMUM_ROM_BYTES {
            return Err(ContentError::new(
                path,
                ContentFailure::InvalidSize {
                    bytes: expected,
                    minimum: core.minimum_rom_bytes(),
                },
            ));
        }

        let maximum_read = u64::try_from(MAXIMUM_ROM_BYTES)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut bytes = Vec::with_capacity(expected);
        file.by_ref()
            .take(maximum_read)
            .read_to_end(&mut bytes)
            .map_err(|source| ContentError::new(path, ContentFailure::Read { source }))?;
        if bytes.len() != expected {
            return Err(ContentError::new(
                path,
                ContentFailure::Changed {
                    expected,
                    actual: bytes.len(),
                },
            ));
        }

        Ok(Self {
            core,
            path: path.to_owned(),
            bytes,
        })
    }

    /// Core selected for this content image.
    #[must_use]
    pub const fn core(&self) -> LibretroCore {
        self.core
    }

    /// Original filesystem path retained for libretro metadata and saves.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "PathBuf deref is not const on the supported Rust toolchain"
    )]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Complete immutable content bytes.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec slice access is not const on the supported Rust toolchain"
    )]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl fmt::Debug for Content {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Content")
            .field("core", &self.core)
            .field("path", &self.path)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// Content loading failure with its source path retained.
#[derive(Debug)]
pub struct ContentError {
    path: PathBuf,
    failure: ContentFailure,
}

impl ContentError {
    fn new(path: &Path, failure: ContentFailure) -> Self {
        Self {
            path: path.to_owned(),
            failure,
        }
    }
}

impl fmt::Display for ContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.failure {
            ContentFailure::WrongExtension { core } => {
                write!(
                    formatter,
                    "{} content {} needs one of these lowercase extensions: ",
                    core.system_name(),
                    self.path.display()
                )?;
                for (index, extension) in core.extensions().iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, ".{extension}")?;
                }
                Ok(())
            }
            ContentFailure::Open { source } => {
                write!(
                    formatter,
                    "cannot open content {}: {source}",
                    self.path.display()
                )
            }
            ContentFailure::Read { source } => {
                write!(
                    formatter,
                    "cannot read content {}: {source}",
                    self.path.display()
                )
            }
            ContentFailure::NotRegular => {
                write!(
                    formatter,
                    "content {} is not a regular file",
                    self.path.display()
                )
            }
            ContentFailure::InvalidSize { bytes, minimum } => write!(
                formatter,
                "content {} contains {bytes} bytes; expected {minimum} through {MAXIMUM_ROM_BYTES}",
                self.path.display()
            ),
            ContentFailure::Changed { expected, actual } => write!(
                formatter,
                "content {} changed while reading: expected {expected} bytes, read {actual}",
                self.path.display()
            ),
        }
    }
}

impl Error for ContentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.failure {
            ContentFailure::Open { source } | ContentFailure::Read { source } => Some(source),
            ContentFailure::WrongExtension { .. }
            | ContentFailure::NotRegular
            | ContentFailure::InvalidSize { .. }
            | ContentFailure::Changed { .. } => None,
        }
    }
}

#[derive(Debug)]
enum ContentFailure {
    WrongExtension { core: LibretroCore },
    Open { source: io::Error },
    Read { source: io::Error },
    NotRegular,
    InvalidSize { bytes: usize, minimum: usize },
    Changed { expected: usize, actual: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn every_core_loads_its_canonical_content_extensions() {
        let Ok(directory) = tempfile::tempdir() else {
            return;
        };
        let fixtures = [
            (LibretroCore::Fceumm, "game.nes", 16),
            (LibretroCore::Gambatte, "game.gb", 0x150),
            (LibretroCore::Gambatte, "game.gbc", 0x150),
            (LibretroCore::Fuse, "game.tap", 4),
        ];
        for (core, filename, size) in fixtures {
            let path = directory.path().join(filename);
            let expected = vec![u8::try_from(size & 0xff).unwrap_or(0); size];
            assert!(fs::write(&path, &expected).is_ok());
            let content = Content::load(core, &path).expect("canonical bounded content loads");
            assert_eq!(content.core(), core);
            assert_eq!(content.path(), path);
            assert_eq!(content.bytes(), expected);
            assert!(format!("{content:?}").contains(&format!("bytes: {size}")));
        }
    }

    #[test]
    fn wrong_or_noncanonical_extensions_are_rejected_before_open() {
        let missing = Path::new("missing.NES");
        let error = Content::load(LibretroCore::Fceumm, missing)
            .expect_err("uppercase extension is not canonical");
        assert!(error.to_string().contains(".nes"));
        assert!(Content::load(LibretroCore::Fuse, Path::new("missing.nes")).is_err());
        assert!(Content::load(LibretroCore::Gambatte, Path::new("missing")).is_err());
    }

    #[test]
    fn unsafe_types_and_sizes_are_rejected() {
        let Ok(directory) = tempfile::tempdir() else {
            return;
        };
        let short = directory.path().join("short.nes");
        assert!(fs::write(&short, [0_u8; 15]).is_ok());
        assert!(Content::load(LibretroCore::Fceumm, &short).is_err());

        let oversized = directory.path().join("oversized.tap");
        assert!(fs::write(&oversized, vec![0_u8; MAXIMUM_ROM_BYTES + 1]).is_ok());
        assert!(Content::load(LibretroCore::Fuse, &oversized).is_err());

        let actual = directory.path().join("actual.nes");
        let alias = directory.path().join("alias.nes");
        assert!(fs::write(&actual, [0_u8; 16]).is_ok());
        assert!(symlink(&actual, &alias).is_ok());
        assert!(Content::load(LibretroCore::Fceumm, &alias).is_err());

        let directory_path = directory.path().join("directory.tap");
        assert!(fs::create_dir(&directory_path).is_ok());
        assert!(Content::load(LibretroCore::Fuse, &directory_path).is_err());
    }
}
