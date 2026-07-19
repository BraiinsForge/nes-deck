//! Idempotent installation of the Retro Deck compositor scene.

use std::{
    ffi::OsString,
    fmt,
    fmt::Write as _,
    io,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

use crate::file::{FileError, atomic_write, install_exclusive, read_bounded_regular};

const RETRO_DECK_WIDGET_UID: &str = "73219c9d-f1ef-41dc-960c-d0711e42a6ac";
const MAXIMUM_BMC_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
const BMC_DIRECTORY_MODE: u32 = 0o700;

/// Whether compositor configuration changed during scene installation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneInstall {
    /// A new fullscreen Retro Deck scene was appended.
    Installed,
    /// A scene already contained the Retro Deck widget type.
    AlreadyPresent,
}

/// Install one fullscreen Retro Deck scene while preserving unrelated JSON.
///
/// The original file is copied once to a sibling `.retro-deck.bak` file. Both
/// reads and writes reject symlinked path components, and the replacement is
/// flushed before it is renamed over the compositor configuration.
///
/// # Errors
///
/// Returns [`BmcError`] for unsafe files, excessive or malformed JSON,
/// operating-system entropy failure, or durable storage errors.
pub fn install_scene(path: &Path) -> Result<SceneInstall, BmcError> {
    let source = read_bounded_regular(path, MAXIMUM_BMC_CONFIG_BYTES).map_err(map_file_error)?;
    if source.contents.is_empty() {
        return Err(BmcError::Invalid("configuration is empty"));
    }
    let mut configuration: Value = serde_json::from_slice(&source.contents)?;
    let root = configuration
        .as_object_mut()
        .ok_or(BmcError::Invalid("configuration root is not an object"))?;
    let scenes_value = root
        .get_mut("scenes")
        .ok_or(BmcError::Invalid("configuration has no scene list"))?;
    if scenes_value.is_null() {
        *scenes_value = Value::Array(Vec::new());
    }
    let scenes = scenes_value
        .as_array_mut()
        .ok_or(BmcError::Invalid("scene list is not an array"))?;
    for scene in scenes.iter() {
        if scene_contains_retro_deck(scene)? {
            return Ok(SceneInstall::AlreadyPresent);
        }
    }

    scenes.push(json!({
        "id": random_uuid()?,
        "enabled": true,
        "kind": "fullscreen",
        "widgets": [{
            "id": random_uuid()?,
            "row": 0,
            "col": 0,
            "placement": "fullscreen",
            "widget_type_id": RETRO_DECK_WIDGET_UID,
            "viewport_shape": "rectangular",
            "params": {}
        }]
    }));

    let mut updated = serde_json::to_vec_pretty(&configuration)?;
    updated.push(b'\n');
    if u64::try_from(updated.len()).unwrap_or(u64::MAX) > MAXIMUM_BMC_CONFIG_BYTES {
        return Err(BmcError::Invalid(
            "updated configuration exceeds its size limit",
        ));
    }

    let file_mode = source.metadata.mode() & 0o777;
    ensure_backup(&backup_path(path), &source.contents, file_mode)?;
    atomic_write(path, &updated, file_mode, BMC_DIRECTORY_MODE).map_err(map_file_error)?;
    Ok(SceneInstall::Installed)
}

fn scene_contains_retro_deck(scene: &Value) -> Result<bool, BmcError> {
    if scene.is_null() {
        return Ok(false);
    }
    let scene = scene
        .as_object()
        .ok_or(BmcError::Invalid("scene is not an object"))?;
    let Some(widgets) = scene.get("widgets") else {
        return Ok(false);
    };
    if widgets.is_null() {
        return Ok(false);
    }
    let widgets = widgets
        .as_array()
        .ok_or(BmcError::Invalid("scene widget list is not an array"))?;
    for widget in widgets {
        if widget.is_null() {
            continue;
        }
        let widget = widget
            .as_object()
            .ok_or(BmcError::Invalid("scene widget is not an object"))?;
        match widget.get("widget_type_id") {
            Some(Value::String(identifier)) if identifier == RETRO_DECK_WIDGET_UID => {
                return Ok(true);
            }
            Some(Value::String(_) | Value::Null) | None => {}
            Some(_) => {
                return Err(BmcError::Invalid(
                    "scene widget type identifier is not a string",
                ));
            }
        }
    }
    Ok(false)
}

fn random_uuid() -> Result<String, BmcError> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|error| BmcError::Random(error.to_string()))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut encoded = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            encoded.push('-');
        }
        if write!(&mut encoded, "{byte:02x}").is_err() {
            return Err(BmcError::Invalid("UUID could not be encoded"));
        }
    }
    Ok(encoded)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".retro-deck.bak");
    PathBuf::from(name)
}

fn ensure_backup(path: &Path, contents: &[u8], file_mode: u32) -> Result<(), BmcError> {
    match read_bounded_regular(path, MAXIMUM_BMC_CONFIG_BYTES) {
        Ok(_) => Ok(()),
        Err(FileError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            match install_exclusive(path, contents, file_mode, BMC_DIRECTORY_MODE) {
                Ok(()) => Ok(()),
                Err(FileError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                    read_bounded_regular(path, MAXIMUM_BMC_CONFIG_BYTES)
                        .map(|_| ())
                        .map_err(map_file_error)
                }
                Err(error) => Err(map_file_error(error)),
            }
        }
        Err(error) => Err(map_file_error(error)),
    }
}

fn map_file_error(error: FileError) -> BmcError {
    match error {
        FileError::Io(error) => BmcError::Io(error),
        FileError::Unsafe(reason) => BmcError::Invalid(reason),
        FileError::Random(error) => BmcError::Random(error),
    }
}

/// Compositor scene validation or persistence failure.
#[derive(Debug)]
pub enum BmcError {
    /// The configuration or one of its filesystem objects is unsafe.
    Invalid(&'static str),
    /// The configuration is not valid JSON or cannot be serialized.
    Json(serde_json::Error),
    /// File access or durable replacement failed.
    Io(io::Error),
    /// Operating-system entropy was unavailable.
    Random(String),
}

impl fmt::Display for BmcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(formatter, "invalid BMC configuration: {reason}"),
            Self::Json(error) => write!(formatter, "BMC configuration JSON failed: {error}"),
            Self::Io(error) => write!(formatter, "BMC configuration I/O failed: {error}"),
            Self::Random(error) => write!(formatter, "cannot generate BMC scene IDs: {error}"),
        }
    }
}

impl std::error::Error for BmcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Invalid(_) | Self::Random(_) => None,
        }
    }
}

impl From<serde_json::Error> for BmcError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{SceneInstall, install_scene};
    use serde_json::Value;
    use std::{
        fs,
        os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink},
    };

    #[test]
    fn installs_once_preserves_configuration_and_keeps_first_backup() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let path = directory.path().join("bmc_config.json");
        let original = br#"{"scenes":[],"sound":{"volume":42}}"#;
        assert!(fs::write(&path, original).is_ok());
        assert!(fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).is_ok());

        assert_eq!(install_scene(&path).ok(), Some(SceneInstall::Installed));
        let installed = fs::read(&path);
        assert!(installed.is_ok());
        let installed = installed.unwrap_or_default();
        let configuration = serde_json::from_slice::<Value>(&installed);
        assert!(configuration.is_ok());
        let configuration = configuration.unwrap_or(Value::Null);
        assert_eq!(
            configuration
                .get("sound")
                .and_then(|sound| sound.get("volume"))
                .and_then(Value::as_u64),
            Some(42)
        );
        let widget = configuration
            .get("scenes")
            .and_then(Value::as_array)
            .and_then(|scenes| scenes.first())
            .and_then(|scene| scene.get("widgets"))
            .and_then(Value::as_array)
            .and_then(|widgets| widgets.first());
        assert!(matches!(
            widget
                .and_then(|value| value.get("widget_type_id"))
                .and_then(Value::as_str),
            Some("73219c9d-f1ef-41dc-960c-d0711e42a6ac")
        ));
        assert!(matches!(
            fs::metadata(&path),
            Ok(metadata) if metadata.mode() & 0o777 == 0o640
        ));
        let backup = path.with_file_name("bmc_config.json.retro-deck.bak");
        assert!(matches!(fs::read(&backup), Ok(contents) if contents == original));

        assert_eq!(
            install_scene(&path).ok(),
            Some(SceneInstall::AlreadyPresent)
        );
        assert!(matches!(fs::read(&path), Ok(contents) if contents == installed));
        assert!(matches!(fs::read(&backup), Ok(contents) if contents == original));
    }

    #[test]
    fn rejects_malformed_shapes_and_symlinked_files() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let path = directory.path().join("bmc.json");
        for malformed in [
            br"[]".as_slice(),
            br"{}".as_slice(),
            br#"{"scenes":{}}"#.as_slice(),
            br#"{"scenes":[{"widgets":[42]}]}"#.as_slice(),
        ] {
            assert!(fs::write(&path, malformed).is_ok());
            assert!(install_scene(&path).is_err());
        }

        let actual = directory.path().join("actual.json");
        assert!(fs::write(&actual, br#"{"scenes":[]}"#).is_ok());
        let link = directory.path().join("link.json");
        assert!(symlink(&actual, &link).is_ok());
        assert!(install_scene(&link).is_err());
    }

    #[test]
    fn refuses_an_unsafe_existing_backup_without_changing_the_source() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let path = directory.path().join("bmc.json");
        let original = br#"{"scenes":[]}"#;
        assert!(fs::write(&path, original).is_ok());
        let victim = directory.path().join("victim");
        assert!(fs::write(&victim, b"untouched").is_ok());
        let backup = directory.path().join("bmc.json.retro-deck.bak");
        assert!(symlink(&victim, &backup).is_ok());

        assert!(install_scene(&path).is_err());
        assert!(matches!(fs::read(&path), Ok(contents) if contents == original));
        assert!(matches!(fs::read(&victim), Ok(contents) if contents == b"untouched"));
    }
}
