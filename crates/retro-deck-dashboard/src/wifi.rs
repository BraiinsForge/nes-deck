//! Pure, bounded state for explicit Wi-Fi profile entry.

use std::fmt;

use crate::MenuCue;

/// Maximum SSID length accepted by the installed profile helper.
pub const MAXIMUM_SSID_BYTES: usize = 32;
/// Minimum WPA passphrase length accepted by the installed profile helper.
pub const MINIMUM_PASSPHRASE_BYTES: usize = 8;
/// Maximum WPA passphrase length accepted by the installed profile helper.
pub const MAXIMUM_PASSPHRASE_BYTES: usize = 63;

/// Editable field receiving on-screen keyboard input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WifiField {
    /// Network name.
    Ssid,
    /// WPA passphrase.
    Passphrase,
}

/// Visible result of the most recent editor operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WifiStatus {
    /// No transient message.
    Clear,
    /// SSID is empty or outside the bounded printable-ASCII contract.
    InvalidSsid,
    /// Passphrase is outside the bounded printable-ASCII contract.
    InvalidPassphrase,
    /// A validated profile is being handed to the isolated writer.
    Saving,
    /// The isolated writer accepted the profile.
    Saved,
    /// The isolated writer rejected or could not store the profile.
    SaveFailed,
}

/// Semantic editor input after touch, keyboard, or controller mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WifiAction {
    /// Close the editor without changing network configuration.
    Close,
    /// Focus one input field.
    SelectField(WifiField),
    /// Switch between letters and punctuation.
    ToggleSymbols,
    /// Switch the letter keyboard between lower and upper case.
    ToggleShift,
    /// Append one printable ASCII byte to the focused field.
    TypeAscii(u8),
    /// Remove the last byte from the focused field.
    Delete,
    /// Validate and request explicit profile storage.
    Save,
}

/// External work requested by the pure editor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WifiEffect {
    /// Return to dashboard settings.
    Close,
    /// Store the currently validated credentials through the isolated helper.
    Save,
}

/// Complete result of one editor action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiTransition {
    /// Whether the next frame differs from the current frame.
    pub redraw: bool,
    /// Optional nonblocking menu sound.
    pub cue: Option<MenuCue>,
    /// Optional external work for the runtime boundary.
    pub effect: Option<WifiEffect>,
}

impl WifiTransition {
    const NONE: Self = Self {
        redraw: false,
        cue: None,
        effect: None,
    };

    const fn redraw(cue: MenuCue) -> Self {
        Self {
            redraw: true,
            cue: Some(cue),
            effect: None,
        }
    }
}

/// Validated credentials borrowed only while submitting one explicit save.
pub struct WifiCredentials<'editor> {
    ssid: &'editor str,
    passphrase: &'editor str,
}

impl WifiCredentials<'_> {
    /// Printable ASCII network name.
    #[must_use]
    pub const fn ssid(&self) -> &str {
        self.ssid
    }

    /// Printable ASCII WPA passphrase.
    #[must_use]
    pub const fn passphrase(&self) -> &str {
        self.passphrase
    }
}

impl fmt::Debug for WifiCredentials<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WifiCredentials")
            .field("ssid", &self.ssid)
            .field("passphrase_bytes", &self.passphrase.len())
            .finish()
    }
}

/// Allocation-free Wi-Fi editor state with bounded credential storage.
pub struct WifiEditor {
    ssid: AsciiField<MAXIMUM_SSID_BYTES>,
    passphrase: AsciiField<MAXIMUM_PASSPHRASE_BYTES>,
    field: WifiField,
    uppercase: bool,
    symbols: bool,
    status: WifiStatus,
}

impl WifiEditor {
    /// Construct an empty editor focused on the SSID field.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ssid: AsciiField::new(),
            passphrase: AsciiField::new(),
            field: WifiField::Ssid,
            uppercase: false,
            symbols: false,
            status: WifiStatus::Clear,
        }
    }

    /// Apply one semantic action without performing external work.
    #[must_use]
    pub fn apply(&mut self, action: WifiAction) -> WifiTransition {
        match action {
            WifiAction::Close => WifiTransition {
                redraw: false,
                cue: Some(MenuCue::Back),
                effect: Some(WifiEffect::Close),
            },
            WifiAction::SelectField(field) => self.select_field(field),
            WifiAction::ToggleSymbols => {
                self.symbols = !self.symbols;
                self.status = WifiStatus::Clear;
                WifiTransition::redraw(MenuCue::Confirm)
            }
            WifiAction::ToggleShift if !self.symbols => {
                self.uppercase = !self.uppercase;
                self.status = WifiStatus::Clear;
                WifiTransition::redraw(MenuCue::Confirm)
            }
            WifiAction::ToggleShift => WifiTransition::NONE,
            WifiAction::TypeAscii(byte) => self.type_ascii(byte),
            WifiAction::Delete => self.delete(),
            WifiAction::Save => self.save(),
        }
    }

    /// Focused field.
    #[must_use]
    pub const fn field(&self) -> WifiField {
        self.field
    }

    /// Whether letter keys are upper case.
    #[must_use]
    pub const fn uppercase(&self) -> bool {
        self.uppercase
    }

    /// Whether punctuation keys replace letter keys.
    #[must_use]
    pub const fn symbols(&self) -> bool {
        self.symbols
    }

    /// Current visible editor status.
    #[must_use]
    pub const fn status(&self) -> WifiStatus {
        self.status
    }

    /// Current printable ASCII SSID.
    #[must_use]
    pub fn ssid(&self) -> &str {
        self.ssid.as_str()
    }

    /// Number of passphrase bytes entered, without exposing their value.
    #[must_use]
    pub const fn passphrase_len(&self) -> usize {
        self.passphrase.len()
    }

    /// Borrow credentials only after the same validation used by Save.
    #[must_use]
    pub fn credentials(&self) -> Option<WifiCredentials<'_>> {
        self.valid_credentials().then(|| WifiCredentials {
            ssid: self.ssid.as_str(),
            passphrase: self.passphrase.as_str(),
        })
    }

    /// Record the isolated writer's result.
    ///
    /// A successful save erases the in-memory passphrase. A failed save keeps
    /// it available for an explicit retry.
    pub fn resolve_save(&mut self, saved: bool) {
        if saved {
            self.passphrase.clear();
            self.status = WifiStatus::Saved;
        } else {
            self.status = WifiStatus::SaveFailed;
        }
    }

    fn select_field(&mut self, field: WifiField) -> WifiTransition {
        let changed = self.field != field || self.status != WifiStatus::Clear;
        self.field = field;
        self.status = WifiStatus::Clear;
        if changed {
            WifiTransition::redraw(MenuCue::Confirm)
        } else {
            WifiTransition::NONE
        }
    }

    fn type_ascii(&mut self, byte: u8) -> WifiTransition {
        if !(b' '..=b'~').contains(&byte) {
            return WifiTransition::NONE;
        }
        let appended = match self.field {
            WifiField::Ssid => self.ssid.push(byte),
            WifiField::Passphrase => self.passphrase.push(byte),
        };
        if !appended {
            return WifiTransition::NONE;
        }
        self.status = WifiStatus::Clear;
        WifiTransition::redraw(MenuCue::Next)
    }

    fn delete(&mut self) -> WifiTransition {
        let deleted = match self.field {
            WifiField::Ssid => self.ssid.pop(),
            WifiField::Passphrase => self.passphrase.pop(),
        };
        if !deleted {
            return WifiTransition::NONE;
        }
        self.status = WifiStatus::Clear;
        WifiTransition::redraw(MenuCue::Back)
    }

    fn save(&mut self) -> WifiTransition {
        if self.status == WifiStatus::Saving {
            return WifiTransition::NONE;
        }
        if self.ssid.is_empty() {
            self.status = WifiStatus::InvalidSsid;
            return WifiTransition::redraw(MenuCue::Back);
        }
        if !valid_passphrase_length(self.passphrase.len()) {
            self.status = WifiStatus::InvalidPassphrase;
            return WifiTransition::redraw(MenuCue::Back);
        }
        self.status = WifiStatus::Saving;
        WifiTransition {
            redraw: true,
            cue: Some(MenuCue::Confirm),
            effect: Some(WifiEffect::Save),
        }
    }

    const fn valid_credentials(&self) -> bool {
        !self.ssid.is_empty() && valid_passphrase_length(self.passphrase.len())
    }
}

impl Default for WifiEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for WifiEditor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WifiEditor")
            .field("ssid", &self.ssid.as_str())
            .field("passphrase_bytes", &self.passphrase.len())
            .field("field", &self.field)
            .field("uppercase", &self.uppercase)
            .field("symbols", &self.symbols)
            .field("status", &self.status)
            .finish()
    }
}

impl Drop for WifiEditor {
    fn drop(&mut self) {
        self.passphrase.clear();
    }
}

const fn valid_passphrase_length(length: usize) -> bool {
    length >= MINIMUM_PASSPHRASE_BYTES && length <= MAXIMUM_PASSPHRASE_BYTES
}

#[derive(Eq, PartialEq)]
struct AsciiField<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    len: usize,
}

impl<const CAPACITY: usize> AsciiField<CAPACITY> {
    const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            len: 0,
        }
    }

    const fn len(&self) -> usize {
        self.len
    }

    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn as_str(&self) -> &str {
        let bytes = self.bytes.get(..self.len).unwrap_or_default();
        std::str::from_utf8(bytes).unwrap_or_default()
    }

    fn push(&mut self, byte: u8) -> bool {
        let Some(slot) = self.bytes.get_mut(self.len) else {
            return false;
        };
        *slot = byte;
        self.len = self.len.saturating_add(1);
        true
    }

    fn pop(&mut self) -> bool {
        let Some(index) = self.len.checked_sub(1) else {
            return false;
        };
        let Some(slot) = self.bytes.get_mut(index) else {
            return false;
        };
        *slot = 0;
        self.len = index;
        true
    }

    fn clear(&mut self) {
        self.bytes.fill(0);
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAXIMUM_PASSPHRASE_BYTES, MAXIMUM_SSID_BYTES, WifiAction, WifiEditor, WifiEffect,
        WifiField, WifiStatus,
    };

    fn type_text(editor: &mut WifiEditor, text: &str) {
        for byte in text.bytes() {
            let _ = editor.apply(WifiAction::TypeAscii(byte));
        }
    }

    #[test]
    fn editor_bounds_fields_and_rejects_nonprinting_input() {
        let mut editor = WifiEditor::new();
        for _ in 0..MAXIMUM_SSID_BYTES.saturating_add(4) {
            let _ = editor.apply(WifiAction::TypeAscii(b'a'));
        }
        assert_eq!(editor.ssid().len(), MAXIMUM_SSID_BYTES);
        assert!(!editor.apply(WifiAction::TypeAscii(b'\n')).redraw);

        let _ = editor.apply(WifiAction::SelectField(WifiField::Passphrase));
        for _ in 0..MAXIMUM_PASSPHRASE_BYTES.saturating_add(4) {
            let _ = editor.apply(WifiAction::TypeAscii(b'p'));
        }
        assert_eq!(editor.passphrase_len(), MAXIMUM_PASSPHRASE_BYTES);
        assert_eq!(editor.field(), WifiField::Passphrase);
    }

    #[test]
    fn save_requires_bounded_credentials_and_suppresses_duplicates() {
        let mut editor = WifiEditor::new();
        let transition = editor.apply(WifiAction::Save);
        assert_eq!(editor.status(), WifiStatus::InvalidSsid);
        assert_eq!(transition.effect, None);

        type_text(&mut editor, "net1");
        let _ = editor.apply(WifiAction::SelectField(WifiField::Passphrase));
        type_text(&mut editor, "short");
        assert_eq!(editor.apply(WifiAction::Save).effect, None);
        assert_eq!(editor.status(), WifiStatus::InvalidPassphrase);

        type_text(&mut editor, "123");
        let transition = editor.apply(WifiAction::Save);
        assert_eq!(transition.effect, Some(WifiEffect::Save));
        assert_eq!(editor.status(), WifiStatus::Saving);
        assert_eq!(editor.apply(WifiAction::Save).effect, None);
        let Some(credentials) = editor.credentials() else {
            return;
        };
        assert_eq!(credentials.ssid(), "net1");
        assert_eq!(credentials.passphrase(), "short123");
    }

    #[test]
    fn diagnostics_redact_password_and_success_erases_it() {
        let mut editor = WifiEditor::new();
        type_text(&mut editor, "studio");
        let _ = editor.apply(WifiAction::SelectField(WifiField::Passphrase));
        type_text(&mut editor, "secret123");
        let diagnostics = format!("{editor:?}");
        assert!(diagnostics.contains("studio"));
        assert!(diagnostics.contains("passphrase_bytes: 9"));
        assert!(!diagnostics.contains("secret123"));

        editor.resolve_save(true);
        assert_eq!(editor.status(), WifiStatus::Saved);
        assert_eq!(editor.passphrase_len(), 0);
        assert!(editor.credentials().is_none());
    }

    #[test]
    fn failed_save_keeps_an_explicit_retry_and_modes_are_closed() {
        let mut editor = WifiEditor::new();
        type_text(&mut editor, "studio");
        let _ = editor.apply(WifiAction::SelectField(WifiField::Passphrase));
        type_text(&mut editor, "secret123");
        let _ = editor.apply(WifiAction::Save);
        editor.resolve_save(false);
        assert_eq!(editor.status(), WifiStatus::SaveFailed);
        assert_eq!(editor.passphrase_len(), 9);

        let _ = editor.apply(WifiAction::ToggleShift);
        assert!(editor.uppercase());
        let _ = editor.apply(WifiAction::ToggleSymbols);
        assert!(editor.symbols());
        assert!(!editor.apply(WifiAction::ToggleShift).redraw);
        assert_eq!(
            editor.apply(WifiAction::Close).effect,
            Some(WifiEffect::Close)
        );
    }
}
