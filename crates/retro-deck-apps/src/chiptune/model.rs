//! Pure chiptune controls and playback-state transitions.

use retro_deck_audio::Volume;
use retro_deck_platform::input::{Button, ButtonEdge};

const GAMEPLAY_INSET: u16 = 16;
const CANVAS_SCALE: u16 = 2;
const CANVAS_WIDTH: u16 = 624;
const CANVAS_HEIGHT: u16 = 224;
const DEFAULT_UNMUTE_PERCENT: u8 = 40;
const VOLUME_STEP: u8 = 5;

const VOLUME_BUTTON: Rect = Rect::new(0, 0, 92, 40);
const CLOSE_BUTTON: Rect = Rect::new(554, 3, 62, 34);
const PLAYBACK_MODE_BUTTON: Rect = Rect::new(113, 177, 92, 34);
const PREVIOUS_FILE_BUTTON: Rect = Rect::new(215, 177, 92, 34);
const PAUSE_BUTTON: Rect = Rect::new(317, 177, 92, 34);
const NEXT_FILE_BUTTON: Rect = Rect::new(419, 177, 92, 34);

/// End-of-track behavior selected in the player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackMode {
    /// Continue through tracks and then files in catalog order.
    LoopAll,
    /// Restart the current track indefinitely.
    LoopOne,
    /// Choose another file and, where applicable, another track.
    Shuffle,
}

impl PlaybackMode {
    const fn next(self) -> Self {
        match self {
            Self::LoopAll => Self::LoopOne,
            Self::LoopOne => Self::Shuffle,
            Self::Shuffle => Self::LoopAll,
        }
    }
}

/// One semantic action accepted from touch or a controller press.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerControl {
    /// Close the player.
    Back,
    /// Select the previous catalog file.
    PreviousFile,
    /// Select the next catalog file.
    NextFile,
    /// Toggle playback pause.
    TogglePause,
    /// Select the previous subsong in a multi-track file.
    PreviousTrack,
    /// Select the next subsong in a multi-track file.
    NextTrack,
    /// Lower volume by one fixed step.
    VolumeDown,
    /// Raise volume by one fixed step.
    VolumeUp,
    /// Toggle muted output while remembering the last audible level.
    ToggleMute,
    /// Cycle loop-all, loop-one, and shuffle behavior.
    CyclePlaybackMode,
}

/// Work requested by a deterministic player-state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerEffect {
    /// The requested control changed nothing.
    None,
    /// Leave the player.
    Exit,
    /// Ask the decoder to open the previous file.
    PreviousFile,
    /// Ask the decoder to open the next file.
    NextFile,
    /// Ask the decoder to select the previous subsong.
    PreviousTrack,
    /// Ask the decoder to select the next subsong.
    NextTrack,
    /// Pause state changed; the BMC audio client must update its gate.
    PauseChanged(bool),
    /// User gain changed; BMC playback and persistent state must update.
    VolumeChanged(Volume),
    /// End-of-track behavior changed.
    PlaybackModeChanged(PlaybackMode),
}

/// Small device-independent state owned by the chiptune application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlayerModel {
    paused: bool,
    playback_mode: PlaybackMode,
    volume: Volume,
    last_audible_percent: u8,
}

impl PlayerModel {
    /// Construct an active loop-all player at the configured volume.
    #[must_use]
    pub const fn new(volume: Volume) -> Self {
        let last_audible_percent = if volume.muted() {
            DEFAULT_UNMUTE_PERCENT
        } else {
            volume.percent()
        };
        Self {
            paused: false,
            playback_mode: PlaybackMode::LoopAll,
            volume,
            last_audible_percent,
        }
    }

    /// Whether decoding and audio submission are paused.
    #[must_use]
    pub const fn paused(self) -> bool {
        self.paused
    }

    /// Current end-of-track behavior.
    #[must_use]
    pub const fn playback_mode(self) -> PlaybackMode {
        self.playback_mode
    }

    /// Current validated gain.
    #[must_use]
    pub const fn volume(self) -> Volume {
        self.volume
    }

    /// Apply one semantic control without performing I/O.
    pub fn apply(&mut self, control: PlayerControl) -> PlayerEffect {
        match control {
            PlayerControl::Back => PlayerEffect::Exit,
            PlayerControl::PreviousFile => PlayerEffect::PreviousFile,
            PlayerControl::NextFile => PlayerEffect::NextFile,
            PlayerControl::PreviousTrack => PlayerEffect::PreviousTrack,
            PlayerControl::NextTrack => PlayerEffect::NextTrack,
            PlayerControl::TogglePause => {
                self.paused = !self.paused;
                PlayerEffect::PauseChanged(self.paused)
            }
            PlayerControl::CyclePlaybackMode => {
                self.playback_mode = self.playback_mode.next();
                PlayerEffect::PlaybackModeChanged(self.playback_mode)
            }
            PlayerControl::VolumeDown => {
                self.set_volume(self.volume.percent().saturating_sub(VOLUME_STEP))
            }
            PlayerControl::VolumeUp => {
                self.set_volume(self.volume.percent().saturating_add(VOLUME_STEP).min(100))
            }
            PlayerControl::ToggleMute => {
                if self.volume.muted() {
                    self.set_volume(self.last_audible_percent)
                } else {
                    self.last_audible_percent = self.volume.percent();
                    self.set_volume(0)
                }
            }
        }
    }

    fn set_volume(&mut self, percent: u8) -> PlayerEffect {
        let next = bounded_volume(percent);
        if next == self.volume {
            return PlayerEffect::None;
        }
        self.volume = next;
        if !next.muted() {
            self.last_audible_percent = next.percent();
        }
        PlayerEffect::VolumeChanged(next)
    }
}

/// Map one controller edge to a player control.
///
/// Releases are ignored. Either controller can operate the player.
#[must_use]
pub const fn controller_control(button: Button, edge: ButtonEdge) -> Option<PlayerControl> {
    if matches!(edge, ButtonEdge::Released) {
        return None;
    }
    match button {
        Button::A => Some(PlayerControl::TogglePause),
        Button::B | Button::Select => Some(PlayerControl::Back),
        Button::Start => Some(PlayerControl::CyclePlaybackMode),
        Button::Up => Some(PlayerControl::VolumeUp),
        Button::Down => Some(PlayerControl::VolumeDown),
        Button::Left => Some(PlayerControl::PreviousFile),
        Button::Right => Some(PlayerControl::NextFile),
        Button::L => Some(PlayerControl::PreviousTrack),
        Button::R => Some(PlayerControl::NextTrack),
    }
}

/// Map a full-screen logical touch coordinate to one visible player control.
#[must_use]
pub const fn touch_control(logical_x: u16, logical_y: u16) -> Option<PlayerControl> {
    let Some(relative_x) = logical_x.checked_sub(GAMEPLAY_INSET) else {
        return None;
    };
    let Some(relative_y) = logical_y.checked_sub(GAMEPLAY_INSET) else {
        return None;
    };
    if relative_x >= CANVAS_WIDTH * CANVAS_SCALE || relative_y >= CANVAS_HEIGHT * CANVAS_SCALE {
        return None;
    }
    let x = relative_x / CANVAS_SCALE;
    let y = relative_y / CANVAS_SCALE;
    if CLOSE_BUTTON.contains(x, y) {
        Some(PlayerControl::Back)
    } else if VOLUME_BUTTON.contains(x, y) {
        Some(PlayerControl::ToggleMute)
    } else if PLAYBACK_MODE_BUTTON.contains(x, y) {
        Some(PlayerControl::CyclePlaybackMode)
    } else if PREVIOUS_FILE_BUTTON.contains(x, y) {
        Some(PlayerControl::PreviousFile)
    } else if PAUSE_BUTTON.contains(x, y) {
        Some(PlayerControl::TogglePause)
    } else if NEXT_FILE_BUTTON.contains(x, y) {
        Some(PlayerControl::NextFile)
    } else {
        None
    }
}

const fn bounded_volume(percent: u8) -> Volume {
    match Volume::new(percent) {
        Some(volume) => volume,
        None => Volume::MUTED,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rect {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

impl Rect {
    const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    const fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.width)
            && y >= self.y
            && y < self.y.saturating_add(self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn volume(percent: u8) -> Volume {
        bounded_volume(percent)
    }

    const fn logical(canvas: u16) -> u16 {
        GAMEPLAY_INSET + canvas * CANVAS_SCALE
    }

    #[test]
    fn controller_mapping_uses_pressed_semantic_buttons() {
        assert_eq!(
            controller_control(Button::A, ButtonEdge::Pressed),
            Some(PlayerControl::TogglePause)
        );
        assert_eq!(
            controller_control(Button::L, ButtonEdge::Pressed),
            Some(PlayerControl::PreviousTrack)
        );
        assert_eq!(
            controller_control(Button::R, ButtonEdge::Pressed),
            Some(PlayerControl::NextTrack)
        );
        assert_eq!(controller_control(Button::Up, ButtonEdge::Released), None);
    }

    #[test]
    fn touch_mapping_matches_only_visible_controls() {
        assert_eq!(
            touch_control(logical(10), logical(14)),
            Some(PlayerControl::ToggleMute)
        );
        assert_eq!(
            touch_control(logical(585), logical(20)),
            Some(PlayerControl::Back)
        );
        assert_eq!(
            touch_control(logical(160), logical(190)),
            Some(PlayerControl::CyclePlaybackMode)
        );
        assert_eq!(
            touch_control(logical(260), logical(190)),
            Some(PlayerControl::PreviousFile)
        );
        assert_eq!(
            touch_control(logical(360), logical(190)),
            Some(PlayerControl::TogglePause)
        );
        assert_eq!(
            touch_control(logical(460), logical(190)),
            Some(PlayerControl::NextFile)
        );
        assert_eq!(touch_control(0, 0), None);
        assert_eq!(touch_control(640, 240), None);
    }

    #[test]
    fn playback_modes_cycle_without_decoder_side_effects() {
        let mut model = PlayerModel::new(volume(42));
        assert_eq!(
            model.apply(PlayerControl::CyclePlaybackMode),
            PlayerEffect::PlaybackModeChanged(PlaybackMode::LoopOne)
        );
        assert_eq!(
            model.apply(PlayerControl::CyclePlaybackMode),
            PlayerEffect::PlaybackModeChanged(PlaybackMode::Shuffle)
        );
        assert_eq!(
            model.apply(PlayerControl::CyclePlaybackMode),
            PlayerEffect::PlaybackModeChanged(PlaybackMode::LoopAll)
        );
    }

    #[test]
    fn mute_restores_the_last_audible_volume() {
        let mut model = PlayerModel::new(volume(42));
        assert_eq!(
            model.apply(PlayerControl::ToggleMute),
            PlayerEffect::VolumeChanged(Volume::MUTED)
        );
        assert_eq!(
            model.apply(PlayerControl::ToggleMute),
            PlayerEffect::VolumeChanged(volume(42))
        );
    }

    #[test]
    fn volume_steps_are_saturating_and_unmute_from_zero() {
        let mut model = PlayerModel::new(Volume::MUTED);
        assert_eq!(
            model.apply(PlayerControl::VolumeUp),
            PlayerEffect::VolumeChanged(volume(5))
        );
        for _ in 0..19 {
            let _effect = model.apply(PlayerControl::VolumeUp);
        }
        assert_eq!(model.volume(), volume(100));
        assert_eq!(model.apply(PlayerControl::VolumeUp), PlayerEffect::None);
        for _ in 0..20 {
            let _effect = model.apply(PlayerControl::VolumeDown);
        }
        assert_eq!(model.volume(), Volume::MUTED);
        assert_eq!(model.apply(PlayerControl::VolumeDown), PlayerEffect::None);
    }

    #[test]
    fn pausing_is_explicit_state_for_the_audio_gate() {
        let mut model = PlayerModel::new(volume(42));
        assert_eq!(
            model.apply(PlayerControl::TogglePause),
            PlayerEffect::PauseChanged(true)
        );
        assert!(model.paused());
        assert_eq!(
            model.apply(PlayerControl::TogglePause),
            PlayerEffect::PauseChanged(false)
        );
        assert!(!model.paused());
    }
}
