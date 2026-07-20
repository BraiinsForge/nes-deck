//! Bounded, symlink-safe discovery of supported music files.

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAXIMUM_FILES: usize = 1_024;
const MAXIMUM_FILE_BYTES: u64 = 16 * 1_024 * 1_024;
const MAXIMUM_DIRECTORY_DEPTH: usize = 4;
const MAXIMUM_INSPECTED_ENTRIES: usize = 16_384;
const SUPPORTED_EXTENSIONS: [&str; 12] = [
    "ay", "gbs", "gym", "hes", "kss", "nsf", "nsfe", "ogg", "sap", "spc", "vgm", "vgz",
];

/// Immutable result of one bounded directory scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiptuneCatalog {
    files: Box<[PathBuf]>,
    inaccessible_entries: usize,
    truncated: bool,
}

impl ChiptuneCatalog {
    /// Discover supported regular files below one real directory.
    ///
    /// Symlinks, hidden entries, empty files, oversized files, unsupported
    /// extensions, and paths deeper than four directories are ignored. An
    /// unreadable child is counted without making the complete player fail.
    /// The root itself must remain a readable, non-symlink directory.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] when the root cannot be inspected or is not a
    /// real directory.
    pub fn scan(root: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let root = root.as_ref();
        let metadata = fs::symlink_metadata(root).map_err(|source| CatalogError::InspectRoot {
            path: root.to_path_buf(),
            source,
        })?;
        if !metadata.file_type().is_dir() {
            return Err(CatalogError::InvalidRoot(root.to_path_buf()));
        }

        let mut state = ScanState::default();
        scan_directory(root, 0, true, &mut state)?;
        state.files.sort_unstable();
        Ok(Self {
            files: state.files.into_boxed_slice(),
            inaccessible_entries: state.inaccessible_entries,
            truncated: state.truncated,
        })
    }

    /// Ordered playable paths found below the root.
    #[must_use]
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Consume the scan result into its ordered playable paths.
    #[must_use]
    pub fn into_files(self) -> Box<[PathBuf]> {
        self.files
    }

    /// Number of child paths that disappeared or could not be inspected.
    #[must_use]
    pub const fn inaccessible_entries(&self) -> usize {
        self.inaccessible_entries
    }

    /// Whether an explicit file or scan-entry bound stopped discovery.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

/// Failure to establish a trustworthy catalog root.
#[derive(Debug)]
pub enum CatalogError {
    /// The root metadata or directory stream could not be read.
    InspectRoot {
        /// Requested catalog root.
        path: PathBuf,
        /// Operating-system failure.
        source: io::Error,
    },
    /// The root is a file, symlink, or another unsupported object.
    InvalidRoot(PathBuf),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InspectRoot { path, source } => {
                write!(formatter, "cannot inspect {}: {source}", path.display())
            }
            Self::InvalidRoot(path) => {
                write!(formatter, "{} is not a real directory", path.display())
            }
        }
    }
}

impl Error for CatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InspectRoot { source, .. } => Some(source),
            Self::InvalidRoot(_) => None,
        }
    }
}

#[derive(Debug, Default)]
struct ScanState {
    files: Vec<PathBuf>,
    inspected_entries: usize,
    inaccessible_entries: usize,
    truncated: bool,
}

fn scan_directory(
    directory: &Path,
    depth: usize,
    root: bool,
    state: &mut ScanState,
) -> Result<(), CatalogError> {
    if state.truncated {
        return Ok(());
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(source) if root => {
            return Err(CatalogError::InspectRoot {
                path: directory.to_path_buf(),
                source,
            });
        }
        Err(_) => {
            state.inaccessible_entries = state.inaccessible_entries.saturating_add(1);
            return Ok(());
        }
    };

    for entry in entries {
        if state.files.len() >= MAXIMUM_FILES
            || state.inspected_entries >= MAXIMUM_INSPECTED_ENTRIES
        {
            state.truncated = true;
            break;
        }
        state.inspected_entries = state.inspected_entries.saturating_add(1);
        let Ok(entry) = entry else {
            state.inaccessible_entries = state.inaccessible_entries.saturating_add(1);
            continue;
        };
        if is_hidden(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            state.inaccessible_entries = state.inaccessible_entries.saturating_add(1);
            continue;
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if depth < MAXIMUM_DIRECTORY_DEPTH {
                scan_directory(&path, depth + 1, false, state)?;
            }
            continue;
        }
        if file_type.is_file()
            && metadata.len() > 0
            && metadata.len() <= MAXIMUM_FILE_BYTES
            && supported_extension(&path)
        {
            state.files.push(path);
        }
    }
    Ok(())
}

fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

fn supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "retro-deck-chiptune-catalog-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("catalog fixture directory is created");
            Self { root }
        }

        fn file(&self, relative: &str, length: u64) -> PathBuf {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("catalog fixture parents are created");
            }
            let file = File::create(&path).expect("catalog fixture file is created");
            file.set_len(length)
                .expect("catalog fixture file length is set");
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn supported_regular_files_are_sorted_without_following_symlinks() {
        let fixture = Fixture::new();
        let second = fixture.file("nested/B.NSF", 1);
        let first = fixture.file("a.ogg", 1);
        let _empty = fixture.file("empty.vgm", 0);
        let _unsupported = fixture.file("notes.txt", 1);
        let _hidden = fixture.file(".hidden.gbs", 1);
        symlink(&first, fixture.root.join("linked.ogg"))
            .expect("catalog fixture symlink is created");

        let catalog = ChiptuneCatalog::scan(&fixture.root).expect("catalog scan succeeds");

        assert_eq!(catalog.files(), &[first, second]);
        assert_eq!(catalog.inaccessible_entries(), 0);
        assert!(!catalog.truncated());
    }

    #[test]
    fn depth_and_size_bounds_exclude_untrusted_payloads() {
        let fixture = Fixture::new();
        let accepted = fixture.file("one/two/three/four/accepted.spc", 1);
        let _too_deep = fixture.file("one/two/three/four/five/deep.spc", 1);
        let _too_large = fixture.file("large.ogg", MAXIMUM_FILE_BYTES + 1);

        let catalog = ChiptuneCatalog::scan(&fixture.root).expect("catalog scan succeeds");

        assert_eq!(catalog.files(), &[accepted]);
    }

    #[test]
    fn a_symlink_or_regular_file_cannot_become_the_catalog_root() {
        let fixture = Fixture::new();
        let file = fixture.file("song.ogg", 1);
        let link = fixture.root.with_extension("link");
        symlink(&fixture.root, &link).expect("catalog root symlink is created");

        assert!(matches!(
            ChiptuneCatalog::scan(&file),
            Err(CatalogError::InvalidRoot(path)) if path == file
        ));
        assert!(matches!(
            ChiptuneCatalog::scan(&link),
            Err(CatalogError::InvalidRoot(path)) if path == link
        ));
        fs::remove_file(link).expect("catalog root symlink is removed");
    }
}
