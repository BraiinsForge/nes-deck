//! Pure lifecycle for one bounded Wi-Fi editor session.

use crate::{
    WifiAction, WifiCredentials, WifiEditor, WifiStatus, WifiTransition, WifiWriterRequestId,
};

/// Optional editor plus the identity of its one in-flight profile save.
#[derive(Debug, Default)]
pub struct WifiSession {
    editor: Option<WifiEditor>,
    pending: Option<WifiWriterRequestId>,
}

impl WifiSession {
    /// Construct a closed editor session.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            editor: None,
            pending: None,
        }
    }

    /// Open a fresh empty editor, preserving an already open session.
    pub fn open(&mut self) -> bool {
        if self.editor.is_some() {
            return false;
        }
        self.editor = Some(WifiEditor::new());
        self.pending = None;
        true
    }

    /// Close the editor, erase its passphrase, and detach any old completion.
    pub fn close(&mut self) -> bool {
        self.pending = None;
        self.editor.take().is_some()
    }

    /// Whether the modal editor currently owns dashboard input and rendering.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.editor.is_some()
    }

    /// Borrow the open editor for rendering.
    #[must_use]
    pub const fn editor(&self) -> Option<&WifiEditor> {
        self.editor.as_ref()
    }

    /// Apply one semantic editor action when the modal is open.
    #[must_use]
    pub fn apply(&mut self, action: WifiAction) -> Option<WifiTransition> {
        self.editor.as_mut().map(|editor| editor.apply(action))
    }

    /// Borrow validated credentials only for immediate writer submission.
    #[must_use]
    pub fn credentials(&self) -> Option<WifiCredentials<'_>> {
        self.editor.as_ref()?.credentials()
    }

    /// Attach the identity returned for the editor's current Saving state.
    pub fn mark_queued(&mut self, request: WifiWriterRequestId) -> bool {
        if self.pending.is_some()
            || !self
                .editor
                .as_ref()
                .is_some_and(|editor| editor.status() == WifiStatus::Saving)
        {
            return false;
        }
        self.pending = Some(request);
        true
    }

    /// Turn the current Saving state into a visible retryable failure.
    pub fn reject_save(&mut self) -> bool {
        self.pending = None;
        self.editor
            .as_mut()
            .is_some_and(|editor| editor.resolve_save(false))
    }

    /// Apply a worker completion only to the editor that submitted its ID.
    pub fn resolve_save(&mut self, request: WifiWriterRequestId, saved: bool) -> bool {
        if self.pending != Some(request) {
            return false;
        }
        self.pending = None;
        self.editor
            .as_mut()
            .is_some_and(|editor| editor.resolve_save(saved))
    }
}

#[cfg(test)]
mod tests {
    use super::WifiSession;
    use crate::{WifiAction, WifiEffect, WifiField, WifiStatus, WifiWriterRequestId};

    #[test]
    fn close_and_reopen_detach_old_worker_completions() {
        let mut session = WifiSession::new();
        assert!(session.open());
        prepare_save(&mut session, "first-net", "firstpass");
        let first = WifiWriterRequestId::from_test_serial(1);
        assert!(session.mark_queued(first));
        assert!(session.close());

        assert!(session.open());
        prepare_save(&mut session, "second-net", "secondpass");
        let second = WifiWriterRequestId::from_test_serial(2);
        assert!(session.mark_queued(second));
        assert!(!session.resolve_save(first, true));
        assert_eq!(
            session.editor().map(crate::WifiEditor::status),
            Some(WifiStatus::Saving)
        );
        assert_eq!(
            session.editor().map(crate::WifiEditor::passphrase_len),
            Some(10)
        );

        assert!(session.resolve_save(second, true));
        assert_eq!(
            session.editor().map(crate::WifiEditor::status),
            Some(WifiStatus::Saved)
        );
        assert_eq!(
            session.editor().map(crate::WifiEditor::passphrase_len),
            Some(0)
        );
    }

    #[test]
    fn rejected_save_stays_retryable_and_duplicate_open_preserves_input() {
        let mut session = WifiSession::new();
        assert!(session.open());
        for byte in b"net1" {
            let _transition = session.apply(WifiAction::TypeAscii(*byte));
        }
        assert!(!session.open());
        prepare_password_and_save(&mut session, "secret123");
        assert!(session.reject_save());
        assert_eq!(
            session.editor().map(crate::WifiEditor::status),
            Some(WifiStatus::SaveFailed)
        );
        assert_eq!(
            session.credentials().map(|value| value.ssid()),
            Some("net1")
        );
    }

    fn prepare_save(session: &mut WifiSession, ssid: &str, passphrase: &str) {
        for byte in ssid.bytes() {
            let _transition = session.apply(WifiAction::TypeAscii(byte));
        }
        prepare_password_and_save(session, passphrase);
    }

    fn prepare_password_and_save(session: &mut WifiSession, passphrase: &str) {
        let _transition = session.apply(WifiAction::SelectField(WifiField::Passphrase));
        for byte in passphrase.bytes() {
            let _transition = session.apply(WifiAction::TypeAscii(byte));
        }
        assert_eq!(
            session
                .apply(WifiAction::Save)
                .and_then(|value| value.effect),
            Some(WifiEffect::Save)
        );
    }
}
