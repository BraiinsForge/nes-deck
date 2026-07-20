//! Pure dashboard navigation and settings state machine.

use std::fmt;

use retro_deck_config::{CatalogEntry, CatalogSystem};

use crate::DashboardCatalog;

const VOLUME_STEP: u8 = 5;
const BRIGHTNESS_STEP: u8 = 10;
const MINIMUM_BRIGHTNESS: u8 = 10;
const REBOOT_CONFIRMATION_MILLISECONDS: u64 = 4_000;
/// Compiled audible level used when no valid state exists.
pub const DEFAULT_VOLUME_PERCENT: u8 = 42;
/// Compiled display level used when no valid state exists.
pub const DEFAULT_BRIGHTNESS_PERCENT: u8 = 60;

/// Current top-level dashboard screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    /// Console tabs and game carousel.
    Dashboard,
    /// Device and application settings.
    Settings,
    /// License and attribution crawl.
    Credits,
}

/// One semantic input action after touch, keyboard, or controller mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    /// Select the previous entry or settings control.
    Previous,
    /// Select the next entry or settings control.
    Next,
    /// Select the previous nonempty console category.
    CategoryPrevious,
    /// Select the next nonempty console category.
    CategoryNext,
    /// Activate the currently selected item.
    Confirm,
    /// Close the current modal screen.
    Back,
    /// Open or close settings.
    ToggleSettings,
    /// Open credits from the dashboard.
    ShowCredits,
    /// Select one rendered category by its view index.
    SelectCategory(usize),
    /// Select and activate one rendered entry by its catalog index.
    ActivateEntry(usize),
    /// Select and activate one rendered settings control.
    ActivateSettings(SettingsTarget),
    /// Reduce menu and game volume.
    VolumeDown,
    /// Increase menu and game volume, restoring it when muted.
    VolumeUp,
    /// Toggle mute through the volume label.
    ToggleMute,
}

/// Controller-focusable settings control.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsTarget {
    /// Reduce volume.
    VolumeDown,
    /// Increase volume.
    VolumeUp,
    /// Reduce display brightness.
    BrightnessDown,
    /// Increase display brightness.
    BrightnessUp,
    /// Open the login shell.
    Terminal,
    /// Toggle terminal keyboard layout.
    Keymap,
    /// Open the separately isolated Wi-Fi editor.
    Wifi,
}

impl SettingsTarget {
    const ALL: [Self; 7] = [
        Self::VolumeDown,
        Self::VolumeUp,
        Self::BrightnessDown,
        Self::BrightnessUp,
        Self::Terminal,
        Self::Keymap,
        Self::Wifi,
    ];

    const fn index(self) -> usize {
        match self {
            Self::VolumeDown => 0,
            Self::VolumeUp => 1,
            Self::BrightnessDown => 2,
            Self::BrightnessUp => 3,
            Self::Terminal => 4,
            Self::Keymap => 5,
            Self::Wifi => 6,
        }
    }

    fn adjacent(self, direction: Direction) -> Self {
        let index = self.index();
        let requested = match direction {
            Direction::Previous => index.checked_sub(1).unwrap_or(Self::ALL.len() - 1),
            Direction::Next => index.saturating_add(1) % Self::ALL.len(),
        };
        Self::ALL
            .get(requested)
            .copied()
            .unwrap_or(Self::VolumeDown)
    }
}

/// Terminal keymap selected in settings.
#[cfg_attr(
    feature = "application-wire",
    derive(serde::Deserialize, serde::Serialize)
)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Keymap {
    /// US ANSI key positions.
    #[cfg_attr(feature = "application-wire", serde(rename = "us"))]
    #[default]
    Us,
    /// Czech key positions.
    #[cfg_attr(feature = "application-wire", serde(rename = "cz"))]
    Czech,
}

impl Keymap {
    /// Toggle between the only two installed layouts.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Us => Self::Czech,
            Self::Czech => Self::Us,
        }
    }

    /// Persistent state value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Us => "us",
            Self::Czech => "cz",
        }
    }
}

/// Valid volume plus the last level to restore after mute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeState {
    percent: u8,
    last_audible: u8,
}

impl VolumeState {
    /// Compiled safe startup state.
    pub const DEFAULT: Self = Self {
        percent: DEFAULT_VOLUME_PERCENT,
        last_audible: DEFAULT_VOLUME_PERCENT,
    };

    /// Validate current and restore levels.
    ///
    /// The current value may be muted. The restore level must be audible so
    /// volume-up and a volume-label tap can always unmute deterministically.
    ///
    /// # Errors
    ///
    /// Returns [`VolumeError`] unless `percent` is at most 100 and
    /// `last_audible` is in 1 through 100.
    pub const fn new(percent: u8, last_audible: u8) -> Result<Self, VolumeError> {
        if percent > 100 || last_audible == 0 || last_audible > 100 {
            Err(VolumeError)
        } else {
            Ok(Self {
                percent,
                last_audible: if percent == 0 { last_audible } else { percent },
            })
        }
    }

    /// Audible percentage, or zero when muted.
    #[must_use]
    pub const fn percent(self) -> u8 {
        self.percent
    }

    /// Whether playback is muted.
    #[must_use]
    pub const fn is_muted(self) -> bool {
        self.percent == 0
    }

    const fn decrease(&mut self) -> bool {
        if self.percent == 0 {
            return false;
        }
        self.percent = self.percent.saturating_sub(VOLUME_STEP);
        if self.percent < VOLUME_STEP {
            self.percent = 0;
        }
        if self.percent > 0 {
            self.last_audible = self.percent;
        }
        true
    }

    fn increase(&mut self) -> bool {
        let requested = if self.percent == 0 {
            self.last_audible
        } else {
            self.percent.saturating_add(VOLUME_STEP).min(100)
        };
        if requested == self.percent {
            return false;
        }
        self.percent = requested;
        self.last_audible = requested;
        true
    }

    const fn toggle_mute(&mut self) {
        if self.percent == 0 {
            self.percent = self.last_audible;
        } else {
            self.last_audible = self.percent;
            self.percent = 0;
        }
    }
}

/// A volume or restore percentage is outside its contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeError;

impl fmt::Display for VolumeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("volume must be 0 through 100 with an audible restore level")
    }
}

impl std::error::Error for VolumeError {}

/// Display brightness constrained to installed ten-point steps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Brightness(u8);

impl Brightness {
    /// Compiled safe startup state.
    pub const DEFAULT: Self = Self(DEFAULT_BRIGHTNESS_PERCENT);

    /// Validate one 10-point step from 10 through 100.
    ///
    /// # Errors
    ///
    /// Returns [`BrightnessError`] for unsupported percentages.
    pub const fn new(percent: u8) -> Result<Self, BrightnessError> {
        if percent < MINIMUM_BRIGHTNESS || percent > 100 || percent % BRIGHTNESS_STEP != 0 {
            Err(BrightnessError)
        } else {
            Ok(Self(percent))
        }
    }

    /// Validated brightness percentage.
    #[must_use]
    pub const fn percent(self) -> u8 {
        self.0
    }

    fn decrease(&mut self) -> bool {
        let requested = self
            .0
            .saturating_sub(BRIGHTNESS_STEP)
            .max(MINIMUM_BRIGHTNESS);
        if requested == self.0 {
            false
        } else {
            self.0 = requested;
            true
        }
    }

    fn increase(&mut self) -> bool {
        let requested = self.0.saturating_add(BRIGHTNESS_STEP).min(100);
        if requested == self.0 {
            false
        } else {
            self.0 = requested;
            true
        }
    }
}

/// Brightness is not a ten-point step from 10 through 100.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrightnessError;

impl fmt::Display for BrightnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("brightness must be a ten-point step from 10 through 100")
    }
}

impl std::error::Error for BrightnessError {}

/// Nonblocking sound feedback requested by a state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuCue {
    /// Previous item or category.
    Previous,
    /// Next item or category.
    Next,
    /// Activation or modal opening.
    Confirm,
    /// Modal closing.
    Back,
    /// Volume level changed while audible.
    Volume,
}

/// External action requested by the pure model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    /// Launch one catalog entry by stable owning-catalog index.
    Launch(usize),
    /// Open the login shell through the managed terminal launcher.
    OpenTerminal,
    /// Open the isolated Wi-Fi editor without changing network state here.
    OpenWifi,
}

/// Persistent or device setting that changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingChange {
    /// Persist and apply volume.
    Volume(u8),
    /// Persist and apply brightness.
    Brightness(u8),
    /// Persist the terminal keymap.
    Keymap(Keymap),
}

/// Typed status line derived without allocating user-facing text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    /// No transient status.
    Clear,
    /// Volume reached mute.
    VolumeMuted,
    /// Audible volume changed.
    Volume(u8),
    /// Brightness changed.
    Brightness(u8),
    /// Terminal keymap changed.
    Keymap(Keymap),
    /// Reboot is armed and needs one more activation before its deadline.
    RebootConfirmation,
}

/// Complete result of one input action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Transition {
    /// Whether the next frame differs from the current frame.
    pub redraw: bool,
    /// Optional asynchronous menu sound request.
    pub cue: Option<MenuCue>,
    /// Optional process or isolated-subsystem request.
    pub intent: Option<Intent>,
    /// Optional durable or device setting update.
    pub setting: Option<SettingChange>,
}

impl Transition {
    const NONE: Self = Self {
        redraw: false,
        cue: None,
        intent: None,
        setting: None,
    };

    const fn redraw(cue: MenuCue) -> Self {
        Self {
            redraw: true,
            cue: Some(cue),
            intent: None,
            setting: None,
        }
    }
}

/// Pure dashboard state independent of display, input, audio, or filesystem I/O.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardModel {
    catalog: DashboardCatalog,
    screen: Screen,
    active_category: usize,
    selected_positions: Vec<usize>,
    settings_target: SettingsTarget,
    volume: VolumeState,
    brightness: Brightness,
    keymap: Keymap,
    status: Status,
    reboot_confirmation: Option<RebootConfirmation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RebootConfirmation {
    entry_index: usize,
    expires_at_ms: u64,
}

impl DashboardModel {
    /// Construct deterministic dashboard state from validated startup values.
    #[must_use]
    pub fn new(
        catalog: DashboardCatalog,
        volume: VolumeState,
        brightness: Brightness,
        keymap: Keymap,
    ) -> Self {
        let selected_positions = vec![0; catalog.categories().len()];
        Self {
            catalog,
            screen: Screen::Dashboard,
            active_category: 0,
            selected_positions,
            settings_target: SettingsTarget::VolumeDown,
            volume,
            brightness,
            keymap,
            status: Status::Clear,
            reboot_confirmation: None,
        }
    }

    /// Apply one semantic input action without performing external work.
    #[must_use]
    pub fn apply(&mut self, action: Action) -> Transition {
        self.apply_at(action, 0)
    }

    /// Apply one semantic action at a caller-supplied monotonic time.
    ///
    /// Production runtimes use this form so reboot confirmation expires even
    /// when wall-clock time changes.
    #[must_use]
    pub fn apply_at(&mut self, action: Action, monotonic_ms: u64) -> Transition {
        let mut redraw = self.expire_reboot_confirmation(monotonic_ms);
        let reboot_target = self.reboot_target_for_action(action);
        if self
            .reboot_confirmation
            .is_some_and(|confirmation| Some(confirmation.entry_index) != reboot_target)
        {
            redraw |= self.cancel_reboot_confirmation();
        }
        let mut transition = match action {
            Action::Previous => self.move_selection(Direction::Previous),
            Action::Next => self.move_selection(Direction::Next),
            Action::CategoryPrevious => self.move_category(Direction::Previous),
            Action::CategoryNext => self.move_category(Direction::Next),
            Action::Confirm => self.confirm(monotonic_ms),
            Action::Back => self.back(),
            Action::ToggleSettings => self.toggle_settings(),
            Action::ShowCredits => self.show_credits(),
            Action::SelectCategory(index) => self.select_category(index),
            Action::ActivateEntry(index) => self.activate_entry(index, monotonic_ms),
            Action::ActivateSettings(target) => self.activate_settings(target),
            Action::VolumeDown => self.change_volume(Direction::Previous),
            Action::VolumeUp => self.change_volume(Direction::Next),
            Action::ToggleMute => self.toggle_mute(),
        };
        transition.redraw |= redraw;
        transition
    }

    /// Expire a pending reboot confirmation at a monotonic deadline.
    ///
    /// Returns whether the visible status changed.
    pub fn advance_time(&mut self, monotonic_ms: u64) -> bool {
        self.expire_reboot_confirmation(monotonic_ms)
    }

    /// Immutable combined catalog used by renderer and launcher.
    #[must_use]
    pub const fn catalog(&self) -> &DashboardCatalog {
        &self.catalog
    }

    /// Current top-level screen.
    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.screen
    }

    /// Index of the current entry-bearing category.
    #[must_use]
    pub const fn active_category_index(&self) -> usize {
        self.active_category
    }

    /// Current category, if the model invariant has not been corrupted.
    #[must_use]
    pub fn active_category(&self) -> Option<&crate::Category> {
        self.catalog.categories().get(self.active_category)
    }

    /// Selected position within the active category.
    #[must_use]
    pub fn selected_position(&self) -> usize {
        self.selected_positions
            .get(self.active_category)
            .copied()
            .unwrap_or_default()
    }

    /// Selected entry and its stable catalog index.
    #[must_use]
    pub fn selected_entry(&self) -> Option<(usize, &CatalogEntry)> {
        let index = self
            .active_category()?
            .entry_indices()
            .get(self.selected_position())
            .copied()?;
        self.catalog.entry(index).map(|entry| (index, entry))
    }

    /// Current settings focus.
    #[must_use]
    pub const fn settings_target(&self) -> SettingsTarget {
        self.settings_target
    }

    /// Current volume state.
    #[must_use]
    pub const fn volume(&self) -> VolumeState {
        self.volume
    }

    /// Current display brightness.
    #[must_use]
    pub const fn brightness(&self) -> Brightness {
        self.brightness
    }

    /// Current terminal keymap.
    #[must_use]
    pub const fn keymap(&self) -> Keymap {
        self.keymap
    }

    /// Current typed transient status.
    #[must_use]
    pub const fn status(&self) -> Status {
        self.status
    }

    /// Adopt a volume value already persisted by a managed child.
    ///
    /// Returns whether the dashboard-visible value changed. This performs no
    /// filesystem or audio operation and emits no persistence effect.
    pub fn adopt_volume(&mut self, volume: VolumeState) -> bool {
        if self.volume == volume {
            return false;
        }
        self.volume = volume;
        self.status = if volume.is_muted() {
            Status::VolumeMuted
        } else {
            Status::Volume(volume.percent())
        };
        true
    }

    fn move_selection(&mut self, direction: Direction) -> Transition {
        if self.screen == Screen::Settings {
            self.settings_target = self.settings_target.adjacent(direction);
            self.status = Status::Clear;
            return Transition::redraw(direction.cue());
        }
        if self.screen != Screen::Dashboard {
            return Transition::NONE;
        }
        let Some(category) = self.active_category() else {
            return Transition::NONE;
        };
        let count = category.len();
        if count < 2 {
            return Transition::NONE;
        }
        let position = self.selected_position();
        let requested = adjacent_index(position, count, direction);
        let Some(slot) = self.selected_positions.get_mut(self.active_category) else {
            return Transition::NONE;
        };
        *slot = requested;
        self.status = Status::Clear;
        Transition::redraw(direction.cue())
    }

    fn move_category(&mut self, direction: Direction) -> Transition {
        if self.screen != Screen::Dashboard || self.catalog.categories().len() < 2 {
            return Transition::NONE;
        }
        self.active_category = adjacent_index(
            self.active_category,
            self.catalog.categories().len(),
            direction,
        );
        self.status = Status::Clear;
        Transition::redraw(direction.cue())
    }

    fn confirm(&mut self, monotonic_ms: u64) -> Transition {
        match self.screen {
            Screen::Dashboard => {
                let Some((index, _entry)) = self.selected_entry() else {
                    return Transition::NONE;
                };
                self.launch_entry(index, monotonic_ms, false)
            }
            Screen::Settings => self.activate_settings(self.settings_target),
            Screen::Credits => Transition::NONE,
        }
    }

    fn back(&mut self) -> Transition {
        if self.screen == Screen::Dashboard {
            return Transition::NONE;
        }
        self.screen = Screen::Dashboard;
        self.status = Status::Clear;
        Transition::redraw(MenuCue::Back)
    }

    fn toggle_settings(&mut self) -> Transition {
        match self.screen {
            Screen::Dashboard => {
                self.screen = Screen::Settings;
                self.settings_target = SettingsTarget::VolumeDown;
                self.status = Status::Clear;
                Transition::redraw(MenuCue::Confirm)
            }
            Screen::Settings => self.back(),
            Screen::Credits => Transition::NONE,
        }
    }

    fn show_credits(&mut self) -> Transition {
        if self.screen != Screen::Dashboard {
            return Transition::NONE;
        }
        self.screen = Screen::Credits;
        self.status = Status::Clear;
        Transition::redraw(MenuCue::Confirm)
    }

    fn select_category(&mut self, index: usize) -> Transition {
        if self.screen != Screen::Dashboard
            || index >= self.catalog.categories().len()
            || index == self.active_category
        {
            return Transition::NONE;
        }
        self.active_category = index;
        self.status = Status::Clear;
        Transition::redraw(MenuCue::Next)
    }

    fn activate_entry(&mut self, index: usize, monotonic_ms: u64) -> Transition {
        if self.screen != Screen::Dashboard {
            return Transition::NONE;
        }
        let Some(position) = self.active_category().and_then(|category| {
            category
                .entry_indices()
                .iter()
                .position(|entry| *entry == index)
        }) else {
            return Transition::NONE;
        };
        let Some(slot) = self.selected_positions.get_mut(self.active_category) else {
            return Transition::NONE;
        };
        let redraw = *slot != position;
        *slot = position;
        self.launch_entry(index, monotonic_ms, redraw)
    }

    fn launch_entry(&mut self, index: usize, monotonic_ms: u64, redraw: bool) -> Transition {
        if !self.is_reboot_entry(index) {
            self.status = Status::Clear;
            return Transition {
                redraw,
                cue: Some(MenuCue::Confirm),
                intent: Some(Intent::Launch(index)),
                setting: None,
            };
        }
        let confirmed = self.reboot_confirmation.is_some_and(|confirmation| {
            confirmation.entry_index == index && monotonic_ms < confirmation.expires_at_ms
        });
        self.reboot_confirmation = if confirmed {
            None
        } else {
            Some(RebootConfirmation {
                entry_index: index,
                expires_at_ms: monotonic_ms.saturating_add(REBOOT_CONFIRMATION_MILLISECONDS),
            })
        };
        self.status = if confirmed {
            Status::Clear
        } else {
            Status::RebootConfirmation
        };
        Transition {
            redraw: true,
            cue: Some(MenuCue::Confirm),
            intent: confirmed.then_some(Intent::Launch(index)),
            setting: None,
        }
    }

    fn reboot_target_for_action(&self, action: Action) -> Option<usize> {
        if self.screen != Screen::Dashboard {
            return None;
        }
        let index = match action {
            Action::Confirm => self.selected_entry().map(|(index, _entry)| index)?,
            Action::ActivateEntry(index) => {
                let belongs_to_active_category = self
                    .active_category()
                    .is_some_and(|category| category.entry_indices().contains(&index));
                if !belongs_to_active_category {
                    return None;
                }
                index
            }
            _ => return None,
        };
        self.is_reboot_entry(index).then_some(index)
    }

    fn is_reboot_entry(&self, index: usize) -> bool {
        self.catalog.entry(index).is_some_and(|entry| {
            entry.system() == CatalogSystem::Deck && entry.identifier() == "reboot"
        })
    }

    fn expire_reboot_confirmation(&mut self, monotonic_ms: u64) -> bool {
        let expired = self
            .reboot_confirmation
            .is_some_and(|confirmation| monotonic_ms >= confirmation.expires_at_ms);
        if expired {
            self.cancel_reboot_confirmation()
        } else {
            false
        }
    }

    fn cancel_reboot_confirmation(&mut self) -> bool {
        if self.reboot_confirmation.take().is_none() {
            return false;
        }
        if self.status == Status::RebootConfirmation {
            self.status = Status::Clear;
        }
        true
    }

    fn activate_settings(&mut self, target: SettingsTarget) -> Transition {
        if self.screen != Screen::Settings {
            return Transition::NONE;
        }
        self.settings_target = target;
        match target {
            SettingsTarget::VolumeDown => self.change_volume(Direction::Previous),
            SettingsTarget::VolumeUp => self.change_volume(Direction::Next),
            SettingsTarget::BrightnessDown => self.change_brightness(Direction::Previous),
            SettingsTarget::BrightnessUp => self.change_brightness(Direction::Next),
            SettingsTarget::Terminal => Transition {
                redraw: false,
                cue: Some(MenuCue::Confirm),
                intent: Some(Intent::OpenTerminal),
                setting: None,
            },
            SettingsTarget::Keymap => {
                self.keymap = self.keymap.toggled();
                self.status = Status::Keymap(self.keymap);
                Transition {
                    redraw: true,
                    cue: Some(MenuCue::Confirm),
                    intent: None,
                    setting: Some(SettingChange::Keymap(self.keymap)),
                }
            }
            SettingsTarget::Wifi => Transition {
                redraw: false,
                cue: Some(MenuCue::Confirm),
                intent: Some(Intent::OpenWifi),
                setting: None,
            },
        }
    }

    fn change_volume(&mut self, direction: Direction) -> Transition {
        let changed = match direction {
            Direction::Previous => self.volume.decrease(),
            Direction::Next => self.volume.increase(),
        };
        if !changed {
            return Transition::NONE;
        }
        self.status = if self.volume.is_muted() {
            Status::VolumeMuted
        } else {
            Status::Volume(self.volume.percent())
        };
        Transition {
            redraw: true,
            cue: (!self.volume.is_muted()).then_some(MenuCue::Volume),
            intent: None,
            setting: Some(SettingChange::Volume(self.volume.percent())),
        }
    }

    fn toggle_mute(&mut self) -> Transition {
        self.volume.toggle_mute();
        self.status = if self.volume.is_muted() {
            Status::VolumeMuted
        } else {
            Status::Volume(self.volume.percent())
        };
        Transition {
            redraw: true,
            cue: (!self.volume.is_muted()).then_some(MenuCue::Volume),
            intent: None,
            setting: Some(SettingChange::Volume(self.volume.percent())),
        }
    }

    fn change_brightness(&mut self, direction: Direction) -> Transition {
        let changed = match direction {
            Direction::Previous => self.brightness.decrease(),
            Direction::Next => self.brightness.increase(),
        };
        if !changed {
            return Transition::NONE;
        }
        self.status = Status::Brightness(self.brightness.percent());
        Transition {
            redraw: true,
            cue: Some(direction.cue()),
            intent: None,
            setting: Some(SettingChange::Brightness(self.brightness.percent())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    Previous,
    Next,
}

impl Direction {
    const fn cue(self) -> MenuCue {
        match self {
            Self::Previous => MenuCue::Previous,
            Self::Next => MenuCue::Next,
        }
    }
}

const fn adjacent_index(position: usize, count: usize, direction: Direction) -> usize {
    if count == 0 {
        return position;
    }
    match direction {
        Direction::Previous => {
            if position == 0 {
                count - 1
            } else {
                position - 1
            }
        }
        Direction::Next => position.saturating_add(1) % count,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Action, Brightness, DashboardModel, Intent, Keymap, MenuCue, Screen, SettingChange,
        SettingsTarget, Status, Transition, VolumeState,
    };
    use crate::DashboardCatalog;
    use retro_deck_config::Catalog;

    const DEPLOYED_CATALOG: &[u8] = include_bytes!("../../../deploy/menu/games.tsv");

    fn model() -> Option<DashboardModel> {
        let catalog = Catalog::parse(DEPLOYED_CATALOG).ok()?;
        let catalog = DashboardCatalog::from_catalog(&catalog).ok()?;
        let volume = VolumeState::new(42, 42).ok()?;
        let brightness = Brightness::new(60).ok()?;
        Some(DashboardModel::new(catalog, volume, brightness, Keymap::Us))
    }

    fn model_with_reboot() -> Option<DashboardModel> {
        let catalog = Catalog::parse(DEPLOYED_CATALOG).ok()?;
        let reboot = retro_deck_config::CatalogEntry::new(
            "reboot",
            "REBOOT",
            retro_deck_config::CatalogSystem::Deck,
            "/mnt/data/nes-deck/games/reboot",
            "#D75F5F",
        )
        .ok()?;
        let catalog =
            DashboardCatalog::from_entries(catalog.entries().iter().cloned().chain([reboot]))
                .ok()?;
        let volume = VolumeState::new(42, 42).ok()?;
        let brightness = Brightness::new(60).ok()?;
        Some(DashboardModel::new(catalog, volume, brightness, Keymap::Us))
    }

    #[test]
    fn categories_wrap_and_retain_their_own_carousel_positions() {
        let Some(mut model) = model() else {
            return;
        };
        assert_eq!(
            model.selected_entry().map(|(_, entry)| entry.identifier()),
            Some("mario")
        );
        assert_eq!(model.apply(Action::Next), Transition::redraw(MenuCue::Next));
        assert_eq!(
            model.selected_entry().map(|(_, entry)| entry.identifier()),
            Some("micro-mages")
        );
        assert_eq!(
            model.apply(Action::CategoryNext),
            Transition::redraw(MenuCue::Next)
        );
        assert_eq!(
            model.active_category().map(crate::Category::label),
            Some("GAME BOY")
        );
        assert_eq!(model.apply(Action::Next), Transition::redraw(MenuCue::Next));
        assert_eq!(
            model.apply(Action::CategoryPrevious),
            Transition::redraw(MenuCue::Previous)
        );
        assert_eq!(model.selected_position(), 1);
        assert_eq!(
            model.apply(Action::CategoryPrevious),
            Transition::redraw(MenuCue::Previous)
        );
        assert_eq!(
            model.active_category().map(crate::Category::label),
            Some("DECK")
        );
    }

    #[test]
    fn confirmation_and_explicit_card_activation_return_stable_indices() {
        let Some(mut model) = model() else {
            return;
        };
        let selected = model.selected_entry().map(|(index, _)| index);
        assert!(selected.is_some());
        let Some(selected) = selected else {
            return;
        };
        assert_eq!(
            model.apply(Action::Confirm),
            Transition {
                redraw: false,
                cue: Some(MenuCue::Confirm),
                intent: Some(Intent::Launch(selected)),
                setting: None,
            }
        );
        assert_eq!(
            model.apply(Action::ActivateEntry(usize::MAX)),
            Transition::NONE
        );
        assert_eq!(
            model.apply(Action::ActivateEntry(2)).intent,
            Some(Intent::Launch(2))
        );
        assert_eq!(model.selected_position(), 2);
    }

    #[test]
    fn reboot_requires_two_matching_activations_before_monotonic_deadline() {
        let Some(mut model) = model_with_reboot() else {
            return;
        };
        let Some(deck_category) = model
            .catalog()
            .categories()
            .iter()
            .position(|category| category.label() == "DECK")
        else {
            return;
        };
        let Some(reboot_index) = model
            .catalog()
            .entries()
            .iter()
            .position(|entry| entry.identifier() == "reboot")
        else {
            return;
        };
        let _ = model.apply(Action::SelectCategory(deck_category));

        let armed = model.apply_at(Action::ActivateEntry(reboot_index), 1_000);
        assert!(armed.redraw);
        assert_eq!(armed.cue, Some(MenuCue::Confirm));
        assert_eq!(armed.intent, None);
        assert_eq!(model.status(), Status::RebootConfirmation);
        assert!(!model.advance_time(4_999));

        let confirmed = model.apply_at(Action::ActivateEntry(reboot_index), 4_999);
        assert_eq!(confirmed.intent, Some(Intent::Launch(reboot_index)));
        assert_eq!(model.status(), Status::Clear);

        let rearmed = model.apply_at(Action::ActivateEntry(reboot_index), 8_000);
        assert_eq!(rearmed.intent, None);
        assert!(model.advance_time(12_000));
        assert_eq!(model.status(), Status::Clear);
        let expired = model.apply_at(Action::ActivateEntry(reboot_index), 12_000);
        assert_eq!(expired.intent, None);
        assert_eq!(model.status(), Status::RebootConfirmation);

        let cancelled = model.apply_at(Action::Previous, 12_001);
        assert!(cancelled.redraw);
        assert_eq!(model.status(), Status::Clear);
        let requires_rearming = model.apply_at(Action::ActivateEntry(reboot_index), 12_002);
        assert_eq!(requires_rearming.intent, None);
    }

    #[test]
    fn modal_navigation_is_explicit_and_fail_closed() {
        let Some(mut model) = model() else {
            return;
        };
        assert_eq!(
            model.apply(Action::ShowCredits),
            Transition::redraw(MenuCue::Confirm)
        );
        assert_eq!(model.screen(), Screen::Credits);
        assert_eq!(model.apply(Action::Next), Transition::NONE);
        assert_eq!(model.apply(Action::ToggleSettings), Transition::NONE);
        assert_eq!(model.apply(Action::Back), Transition::redraw(MenuCue::Back));
        assert_eq!(
            model.apply(Action::ToggleSettings),
            Transition::redraw(MenuCue::Confirm)
        );
        assert_eq!(model.screen(), Screen::Settings);
        assert_eq!(model.settings_target(), SettingsTarget::VolumeDown);
        assert_eq!(
            model.apply(Action::Previous),
            Transition::redraw(MenuCue::Previous)
        );
        assert_eq!(model.settings_target(), SettingsTarget::Wifi);
        assert_eq!(model.apply(Action::Back), Transition::redraw(MenuCue::Back));
        assert_eq!(model.screen(), Screen::Dashboard);
    }

    #[test]
    fn volume_mutes_restores_and_never_requires_audio_on_the_input_path() {
        let Some(mut model) = model() else {
            return;
        };
        assert_eq!(
            model.apply(Action::ToggleMute).setting,
            Some(SettingChange::Volume(0))
        );
        assert_eq!(model.status(), Status::VolumeMuted);
        assert_eq!(model.apply(Action::VolumeDown), Transition::NONE);
        let restored = model.apply(Action::VolumeUp);
        assert_eq!(restored.setting, Some(SettingChange::Volume(42)));
        assert_eq!(restored.cue, Some(MenuCue::Volume));
        assert_eq!(model.status(), Status::Volume(42));
        for _ in 0..20 {
            let _ = model.apply(Action::VolumeUp);
        }
        assert_eq!(model.volume().percent(), 100);
        assert_eq!(model.apply(Action::VolumeUp), Transition::NONE);
    }

    #[test]
    fn child_volume_adoption_changes_memory_without_emitting_an_effect() {
        let Some(mut model) = model() else {
            return;
        };
        let Some(child_volume) = VolumeState::new(65, 65).ok() else {
            return;
        };
        assert!(model.adopt_volume(child_volume));
        assert_eq!(model.volume(), child_volume);
        assert_eq!(model.status(), Status::Volume(65));
        assert!(!model.adopt_volume(child_volume));

        let Some(muted) = VolumeState::new(0, 65).ok() else {
            return;
        };
        assert!(model.adopt_volume(muted));
        assert_eq!(model.status(), Status::VolumeMuted);
        assert_eq!(
            model.apply(Action::VolumeUp).setting,
            Some(SettingChange::Volume(65))
        );
    }

    #[test]
    fn settings_emit_typed_effects_but_perform_no_external_work() {
        let Some(mut model) = model() else {
            return;
        };
        let _ = model.apply(Action::ToggleSettings);
        assert_eq!(
            model
                .apply(Action::ActivateSettings(SettingsTarget::BrightnessUp))
                .setting,
            Some(SettingChange::Brightness(70))
        );
        assert_eq!(
            model
                .apply(Action::ActivateSettings(SettingsTarget::Keymap))
                .setting,
            Some(SettingChange::Keymap(Keymap::Czech))
        );
        assert_eq!(model.keymap().as_str(), "cz");
        assert_eq!(
            model
                .apply(Action::ActivateSettings(SettingsTarget::Terminal))
                .intent,
            Some(Intent::OpenTerminal)
        );
        assert_eq!(
            model
                .apply(Action::ActivateSettings(SettingsTarget::Wifi))
                .intent,
            Some(Intent::OpenWifi)
        );
    }

    #[test]
    fn startup_value_types_reject_ambiguous_state() {
        assert!(VolumeState::new(101, 42).is_err());
        assert!(VolumeState::new(0, 0).is_err());
        assert!(Brightness::new(0).is_err());
        assert!(Brightness::new(65).is_err());
        assert_eq!(Brightness::new(100).map(Brightness::percent), Ok(100));
    }
}
