//! One bounded cover texture for the currently selected carousel entry.

use std::fs::File;
use std::io::{self, Read as _};
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use rustix::fs::{Mode, OFlags, open};

/// Square artwork dimensions used by the approved carousel card.
pub const NATIVE_COVER_SIZE: u32 = 248;
const MAXIMUM_COMPRESSED_BYTES: usize = 4 * 1_024 * 1_024;

/// One cover decoded and center-cropped by `bmc-render` for direct GPU upload.
#[derive(Debug)]
pub struct NativeCover {
    rgba: Vec<u8>,
}

impl NativeCover {
    /// Exact RGBA bytes for [`NATIVE_COVER_SIZE`] square pixels.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// Decode `<directory>/<identifier>.png`, or return `None` when it has not
/// entered the persistent cache yet.
///
/// The identifier is validated independently of the catalog parser. The file
/// is opened without following its final path component and is bounded before
/// BMC's allocation-capped image decoder sees it.
///
/// # Errors
///
/// Returns an error for an unsafe path or identifier, a non-regular or
/// oversized file, an I/O failure other than absence, or invalid image data.
pub fn load_native_cover(directory: &Path, identifier: &str) -> Result<Option<NativeCover>> {
    if !directory.is_absolute()
        || identifier.is_empty()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("unsafe native cover path");
    }
    let path = directory.join(format!("{identifier}.png"));
    let bytes = match read_bounded_regular(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let (rgba, width, height) =
        bmc_render::decode_scaled_to_cover(&bytes, NATIVE_COVER_SIZE, NATIVE_COVER_SIZE)
            .with_context(|| format!("decode {}", path.display()))?;
    if width != NATIVE_COVER_SIZE || height != NATIVE_COVER_SIZE {
        bail!("BMC cover decoder returned unexpected dimensions");
    }
    Ok(Some(NativeCover { rgba }))
}

fn read_bounded_regular(path: &Path) -> io::Result<Vec<u8>> {
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let file = File::from(descriptor);
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cover is not a regular file",
        ));
    }
    let maximum = u64::try_from(MAXIMUM_COMPRESSED_BYTES).unwrap_or(u64::MAX);
    if metadata.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cover exceeds its compressed size limit",
        ));
    }
    let mut bytes =
        Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(MAXIMUM_COMPRESSED_BYTES));
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAXIMUM_COMPRESSED_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cover exceeds its compressed size limit",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> std::path::PathBuf {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "retro-deck-native-cover-{}-{serial}",
            std::process::id()
        ))
    }

    #[test]
    fn decodes_one_bounded_square_and_treats_absence_as_cache_state() {
        let directory = fixture();
        fs::create_dir(&directory).expect("cover directory is created");
        assert!(matches!(load_native_cover(&directory, "missing"), Ok(None)));
        fs::write(
            directory.join("mario.png"),
            include_bytes!("../assets/gear-knekko-09.png"),
        )
        .expect("PNG fixture is written");
        let cover = load_native_cover(&directory, "mario")
            .expect("cover load succeeds")
            .expect("cover exists");
        assert_eq!(
            cover.rgba().len(),
            NATIVE_COVER_SIZE as usize * NATIVE_COVER_SIZE as usize * 4
        );
        let _ignored = fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_identifier_traversal_and_final_symlinks() {
        let directory = fixture();
        fs::create_dir(&directory).expect("cover directory is created");
        assert!(load_native_cover(&directory, "../escape").is_err());
        let target = directory.join("target.png");
        let alias = directory.join("alias.png");
        fs::write(&target, include_bytes!("../assets/gear-knekko-09.png"))
            .expect("target is written");
        symlink(&target, &alias).expect("alias is created");
        assert!(load_native_cover(&directory, "alias").is_err());
        let _ignored = fs::remove_dir_all(directory);
    }
}
