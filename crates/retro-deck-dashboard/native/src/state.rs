//! Pure dashboard catalog navigation.

use retro_deck_config::CatalogEntry;

use crate::{DashboardCatalog, Keymap, VolumeState};

/// One semantic navigation action after touch, keyboard, or controller mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    /// Select the previous entry.
    Previous,
    /// Select the next entry.
    Next,
    /// Select the previous nonempty console category.
    CategoryPrevious,
    /// Select the next nonempty console category.
    CategoryNext,
    /// Select one nonempty console category by its visible tab index.
    CategorySelect(usize),
    /// Activate the currently selected catalog entry.
    Confirm,
}

/// Nonblocking sound feedback requested by a state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuCue {
    /// Previous item or category.
    Previous,
    /// Next item or category.
    Next,
    /// Catalog activation or modal opening.
    Confirm,
    /// Modal closing.
    Back,
}

/// External action requested by the pure model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    /// Launch one catalog entry by stable owning-catalog index.
    Launch(usize),
}

/// Complete result of one navigation action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Transition {
    /// Whether the next frame differs from the current frame.
    pub redraw: bool,
    /// Optional asynchronous menu sound request.
    pub cue: Option<MenuCue>,
    /// Optional managed application request.
    pub intent: Option<Intent>,
}

impl Transition {
    const NONE: Self = Self {
        redraw: false,
        cue: None,
        intent: None,
    };

    const fn redraw(cue: MenuCue) -> Self {
        Self {
            redraw: true,
            cue: Some(cue),
            intent: None,
        }
    }
}

/// Product state independent of display, input, audio, and filesystem I/O.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardModel {
    catalog: DashboardCatalog,
    active_category: usize,
    selected_positions: Vec<usize>,
    volume: VolumeState,
    keymap: Keymap,
}

impl DashboardModel {
    /// Construct deterministic navigation from a validated catalog and launch defaults.
    #[must_use]
    pub fn new(catalog: DashboardCatalog, volume: VolumeState, keymap: Keymap) -> Self {
        let selected_positions = vec![0; catalog.categories().len()];
        Self {
            catalog,
            active_category: 0,
            selected_positions,
            volume,
            keymap,
        }
    }

    /// Apply one semantic navigation action without performing external work.
    #[must_use]
    pub fn apply(&mut self, action: Action) -> Transition {
        match action {
            Action::Previous => self.move_selection(Direction::Previous),
            Action::Next => self.move_selection(Direction::Next),
            Action::CategoryPrevious => self.move_category(Direction::Previous),
            Action::CategoryNext => self.move_category(Direction::Next),
            Action::CategorySelect(index) => self.select_category(index),
            Action::Confirm => self.confirm(),
        }
    }

    /// Immutable combined catalog used by renderer and launcher.
    #[must_use]
    pub const fn catalog(&self) -> &DashboardCatalog {
        &self.catalog
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

    /// Initial game and menu volume until BMC exposes it as a widget setting.
    #[must_use]
    pub const fn volume(&self) -> VolumeState {
        self.volume
    }

    /// Terminal keymap supplied to the managed terminal application.
    #[must_use]
    pub const fn keymap(&self) -> Keymap {
        self.keymap
    }

    fn move_selection(&mut self, direction: Direction) -> Transition {
        let Some(category) = self.active_category() else {
            return Transition::NONE;
        };
        let count = category.len();
        if count < 2 {
            return Transition::NONE;
        }
        let requested = adjacent_index(self.selected_position(), count, direction);
        let Some(slot) = self.selected_positions.get_mut(self.active_category) else {
            return Transition::NONE;
        };
        *slot = requested;
        Transition::redraw(direction.cue())
    }

    fn move_category(&mut self, direction: Direction) -> Transition {
        let count = self.catalog.categories().len();
        if count < 2 {
            return Transition::NONE;
        }
        self.active_category = adjacent_index(self.active_category, count, direction);
        Transition::redraw(direction.cue())
    }

    fn select_category(&mut self, index: usize) -> Transition {
        if index >= self.catalog.categories().len() || index == self.active_category {
            return Transition::NONE;
        }
        self.active_category = index;
        Transition::redraw(MenuCue::Next)
    }

    fn confirm(&self) -> Transition {
        let Some((index, _entry)) = self.selected_entry() else {
            return Transition::NONE;
        };
        Transition {
            redraw: false,
            cue: Some(MenuCue::Confirm),
            intent: Some(Intent::Launch(index)),
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
    use retro_deck_config::Catalog;

    use super::{Action, DashboardModel, Intent, Keymap, MenuCue, Transition, VolumeState};
    use crate::DashboardCatalog;

    const DEPLOYED_CATALOG: &[u8] = include_bytes!("../../../../deploy/menu/games.tsv");

    fn model() -> Option<DashboardModel> {
        let catalog = Catalog::parse(DEPLOYED_CATALOG).ok()?;
        let catalog = DashboardCatalog::from_catalog(&catalog).ok()?;
        Some(DashboardModel::new(
            catalog,
            VolumeState::DEFAULT,
            Keymap::Us,
        ))
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
    fn confirmation_returns_the_stable_catalog_index() {
        let Some(mut model) = model() else {
            return;
        };
        let Some((selected, _entry)) = model.selected_entry() else {
            return;
        };
        assert_eq!(
            model.apply(Action::Confirm),
            Transition {
                redraw: false,
                cue: Some(MenuCue::Confirm),
                intent: Some(Intent::Launch(selected)),
            }
        );
    }

    #[test]
    fn tabs_select_only_existing_categories() {
        let Some(mut model) = model() else {
            return;
        };
        assert_eq!(model.apply(Action::CategorySelect(0)), Transition::NONE);
        assert_eq!(
            model.apply(Action::CategorySelect(1)),
            Transition::redraw(MenuCue::Next)
        );
        assert_eq!(
            model.active_category().map(crate::Category::label),
            Some("GAME BOY")
        );
        assert_eq!(
            model.apply(Action::CategorySelect(usize::MAX)),
            Transition::NONE
        );
    }

    #[test]
    fn model_retains_only_managed_launch_defaults() {
        let Some(model) = model() else {
            return;
        };
        assert_eq!(model.volume(), VolumeState::DEFAULT);
        assert_eq!(model.keymap(), Keymap::Us);
    }
}
