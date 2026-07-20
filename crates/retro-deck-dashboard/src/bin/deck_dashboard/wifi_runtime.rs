//! Isolated Wi-Fi editor and profile-writer lifecycle.

use retro_deck_dashboard::{
    WifiAction, WifiEffect, WifiProfileWriter, WifiWriterPoll, WifiWriterReport, WifiWriterResult,
    WifiWriterSubmit,
};
use retro_deck_platform::input::{Button, ButtonEdge};

use super::{APPLICATION, DashboardRuntime};

const WIFI_HELPER: &str = "/usr/sbin/deck-wifi-profile-add";

impl DashboardRuntime {
    pub(super) fn open_wifi_editor(&mut self) {
        if self.wifi_session.open() {
            self.touch.cancel();
            self.wifi_touch.cancel();
            self.dirty = true;
        }
    }

    pub(super) fn apply_wifi_action(&mut self, action: WifiAction) {
        let Some(transition) = self.wifi_session.apply(action) else {
            return;
        };
        self.dirty |= transition.redraw;
        match transition.effect {
            Some(WifiEffect::Close) => {
                self.wifi_session.close();
                self.wifi_touch.cancel();
                self.dirty = true;
            }
            Some(WifiEffect::Save) => self.submit_wifi_profile(),
            None => {}
        }
        if let (Some(audio), Some(cue)) = (&self.audio, transition.cue) {
            let _outcome = audio.try_play(cue);
        }
    }

    fn submit_wifi_profile(&mut self) {
        let submission = match (self.wifi_writer.as_ref(), self.wifi_session.credentials()) {
            (Some(writer), Some(credentials)) => writer.try_save(&credentials),
            (None, _) => WifiWriterSubmit::Disconnected,
            (Some(_), None) => WifiWriterSubmit::Invalid,
        };
        match submission {
            WifiWriterSubmit::Queued(request) => {
                if !self.wifi_session.mark_queued(request) {
                    eprintln!(
                        "{APPLICATION}: detached an unexpectedly unowned Wi-Fi request {}",
                        request.get()
                    );
                    self.dirty |= self.wifi_session.reject_save();
                }
            }
            WifiWriterSubmit::Busy => {
                eprintln!("{APPLICATION}: Wi-Fi profile writer is busy; save can be retried");
                self.dirty |= self.wifi_session.reject_save();
            }
            WifiWriterSubmit::Invalid => {
                eprintln!("{APPLICATION}: Wi-Fi profile writer rejected invalid field bounds");
                self.dirty |= self.wifi_session.reject_save();
            }
            WifiWriterSubmit::Disconnected => {
                eprintln!("{APPLICATION}: Wi-Fi profile writer is unavailable");
                self.dirty |= self.wifi_session.reject_save();
            }
            WifiWriterSubmit::Exhausted => {
                eprintln!("{APPLICATION}: Wi-Fi profile request identifiers are exhausted");
                self.dirty |= self.wifi_session.reject_save();
            }
        }
    }

    pub(super) fn service_wifi_writer(&mut self) {
        let mut disconnected = false;
        loop {
            let Some(writer) = self.wifi_writer.as_ref() else {
                return;
            };
            match writer.try_result() {
                WifiWriterPoll::Result(WifiWriterResult::Saved { request }) => {
                    if self.wifi_session.resolve_save(request, true) {
                        eprintln!(
                            "{APPLICATION}: Wi-Fi profile request {} was saved",
                            request.get()
                        );
                        self.dirty = true;
                    } else {
                        eprintln!(
                            "{APPLICATION}: ignored detached Wi-Fi completion {}",
                            request.get()
                        );
                    }
                }
                WifiWriterPoll::Result(WifiWriterResult::Failed { request, error }) => {
                    eprintln!(
                        "{APPLICATION}: Wi-Fi profile request {} failed: {error}",
                        request.get()
                    );
                    self.dirty |= self.wifi_session.resolve_save(request, false);
                }
                WifiWriterPoll::Empty => break,
                WifiWriterPoll::Disconnected => {
                    disconnected = true;
                    self.dirty |= self.wifi_session.reject_save();
                    break;
                }
            }
        }
        if disconnected {
            self.finish_wifi_writer();
        }
    }

    pub(super) fn finish_wifi_writer(&mut self) {
        let Some(writer) = self.wifi_writer.take() else {
            return;
        };
        report_wifi_writer_shutdown(writer.shutdown());
    }
}

pub(super) fn start_wifi_writer() -> Option<WifiProfileWriter> {
    match WifiProfileWriter::spawn(WIFI_HELPER) {
        Ok(writer) => Some(writer),
        Err(error) => {
            eprintln!(
                "{APPLICATION}: cannot start Wi-Fi profile writer: {error}; profile saves are disabled"
            );
            None
        }
    }
}

pub(super) const fn wifi_controller_action(button: Button, edge: ButtonEdge) -> Option<WifiAction> {
    match (button, edge) {
        (Button::B, ButtonEdge::Pressed) => Some(WifiAction::Close),
        _ => None,
    }
}

fn report_wifi_writer_shutdown(report: WifiWriterReport) {
    if report.panicked {
        eprintln!("{APPLICATION}: Wi-Fi profile writer panicked during shutdown");
    }
    if report.failed != 0 || report.dropped_results != 0 {
        eprintln!(
            "{APPLICATION}: Wi-Fi profile writer stopped after {} failure(s), with {} result(s) omitted",
            report.failed, report.dropped_results
        );
    }
}

#[cfg(test)]
mod tests {
    use retro_deck_dashboard::WifiAction;
    use retro_deck_platform::input::{Button, ButtonEdge};

    use super::wifi_controller_action;

    #[test]
    fn only_a_committed_b_press_closes_the_touch_editor() {
        assert_eq!(
            wifi_controller_action(Button::B, ButtonEdge::Pressed),
            Some(WifiAction::Close)
        );
        assert_eq!(
            wifi_controller_action(Button::B, ButtonEdge::Released),
            None
        );
        assert_eq!(wifi_controller_action(Button::A, ButtonEdge::Pressed), None);
    }
}
