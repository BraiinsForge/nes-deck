//! Optional installed palette loading with a compiled safe fallback.

use std::fs::File;
use std::io::{self, Read as _};
use std::path::Path;

use retro_deck_config::{MAXIMUM_PALETTE_BYTES, Palette};
use rustix::fs::{Mode, OFlags, open};

/// Load the installed full-RGB palette without making appearance state a
/// dashboard-startup dependency.
///
/// The checked-in TSV is preferred over the compiled default, and a valid
/// owner override takes final precedence. Missing, malformed, oversized, and
/// symlinked optional files are logged and ignored.
#[must_use]
pub fn load_native_palette(base: &Path, override_path: &Path) -> Palette {
    let fallback = Palette::default();
    let base_palette = match read_bounded_regular(base).and_then(|contents| parse_tsv(&contents)) {
        Ok(palette) => palette,
        Err(error) => {
            tracing::warn!(?error, path = %base.display(), "using compiled dashboard palette");
            fallback
        }
    };

    match read_bounded_regular(override_path).and_then(|contents| parse_override(&contents)) {
        Ok(palette) => palette,
        Err(error) if error.kind() == io::ErrorKind::NotFound => base_palette,
        Err(error) => {
            tracing::warn!(
                ?error,
                path = %override_path.display(),
                "ignoring invalid dashboard palette override"
            );
            base_palette
        }
    }
}

fn parse_tsv(contents: &[u8]) -> io::Result<Palette> {
    Palette::parse_tsv(contents).map_err(invalid_data)
}

fn parse_override(contents: &[u8]) -> io::Result<Palette> {
    Palette::parse_override(contents).map_err(invalid_data)
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
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
        return Err(invalid_data("palette is not a regular file"));
    }
    let maximum = u64::try_from(MAXIMUM_PALETTE_BYTES).unwrap_or(u64::MAX);
    if metadata.len() > maximum {
        return Err(invalid_data("palette exceeds its size limit"));
    }
    let mut contents =
        Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(MAXIMUM_PALETTE_BYTES));
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut contents)?;
    if contents.len() > MAXIMUM_PALETTE_BYTES {
        return Err(invalid_data("palette exceeds its size limit"));
    }
    Ok(contents)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use retro_deck_config::PaletteRole;

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn fixture(name: &str) -> std::path::PathBuf {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "retro-deck-native-theme-{}-{serial}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn override_wins_and_invalid_optional_files_fall_back() {
        let base = fixture("base.tsv");
        let override_path = fixture("override.sexp");
        fs::write(&base, include_bytes!("../../../../deploy/menu/palette.tsv"))
            .expect("base palette is written");

        let mut pairs = Palette::default()
            .fields()
            .into_iter()
            .map(|field| (field.name.to_owned(), field.value))
            .collect::<Vec<_>>();
        let accent = pairs
            .iter_mut()
            .find(|(name, _)| name == PaletteRole::Accent.as_str())
            .expect("accent field exists");
        accent.1 = "#010203".to_owned();
        let custom = Palette::from_pairs(pairs).expect("custom palette is valid");
        fs::write(&override_path, custom.encode_override()).expect("override is written");
        assert_eq!(
            load_native_palette(&base, &override_path).color(PaletteRole::Accent),
            custom.color(PaletteRole::Accent)
        );

        fs::write(&override_path, b"broken").expect("broken override is written");
        assert_eq!(
            load_native_palette(&base, &override_path),
            Palette::default()
        );
        let _ignored = fs::remove_file(base);
        let _ignored = fs::remove_file(override_path);
    }
}
