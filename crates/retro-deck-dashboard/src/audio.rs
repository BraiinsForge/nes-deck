//! Bounded menu cue definitions shared by dashboard runtimes.

use std::sync::OnceLock;

use retro_deck_audio::ToneNote;

use crate::MenuCue;

const VOLUME_SPEC: [(u32, u32); 2] = [(660, 60), (880, 60)];
const PREVIOUS_SPEC: [(u32, u32); 1] = [(523, 35)];
const NEXT_SPEC: [(u32, u32); 1] = [(659, 35)];
const CONFIRM_SPEC: [(u32, u32); 2] = [(659, 25), (880, 30)];
const BACK_SPEC: [(u32, u32); 2] = [(659, 25), (440, 30)];

/// Return the validated notes for one menu cue.
///
/// Validation and allocation happen once per cue kind. Audio device ownership
/// remains the responsibility of the platform worker, outside the input path.
#[must_use]
pub fn menu_notes(cue: MenuCue) -> &'static [ToneNote] {
    static VOLUME: OnceLock<Vec<ToneNote>> = OnceLock::new();
    static PREVIOUS: OnceLock<Vec<ToneNote>> = OnceLock::new();
    static NEXT: OnceLock<Vec<ToneNote>> = OnceLock::new();
    static CONFIRM: OnceLock<Vec<ToneNote>> = OnceLock::new();
    static BACK: OnceLock<Vec<ToneNote>> = OnceLock::new();

    match cue {
        MenuCue::Volume => VOLUME.get_or_init(|| validated_notes(&VOLUME_SPEC)),
        MenuCue::Previous => PREVIOUS.get_or_init(|| validated_notes(&PREVIOUS_SPEC)),
        MenuCue::Next => NEXT.get_or_init(|| validated_notes(&NEXT_SPEC)),
        MenuCue::Confirm => CONFIRM.get_or_init(|| validated_notes(&CONFIRM_SPEC)),
        MenuCue::Back => BACK.get_or_init(|| validated_notes(&BACK_SPEC)),
    }
    .as_slice()
}

fn validated_notes(specification: &[(u32, u32)]) -> Vec<ToneNote> {
    specification
        .iter()
        .filter_map(|(frequency, duration)| ToneNote::new(*frequency, *duration))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{MenuCue, menu_notes};

    #[test]
    fn cues_preserve_the_existing_dashboard_sequences() {
        let cases: &[(MenuCue, &[(u32, u32)])] = &[
            (MenuCue::Volume, &[(660, 60), (880, 60)]),
            (MenuCue::Previous, &[(523, 35)]),
            (MenuCue::Next, &[(659, 35)]),
            (MenuCue::Confirm, &[(659, 25), (880, 30)]),
            (MenuCue::Back, &[(659, 25), (440, 30)]),
        ];

        for (cue, expected) in cases {
            let actual: Vec<_> = menu_notes(*cue)
                .iter()
                .map(|note| (note.frequency_hz(), note.duration_ms()))
                .collect();
            assert_eq!(actual, *expected);
        }
    }
}
