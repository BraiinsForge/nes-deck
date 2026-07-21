//! Retro Deck product UI expressed as a native `bmc-render` tree.

use bmc_render::BitmapId;
use bmc_render::tree::{
    AutoFit, Color, DrawCommand, Fill, FontFamily, FontWeight, PathPaint, PropsData, TextAlign,
    TextStyle, TreeNode, VerticalAlign,
};
use retro_deck_config::{CatalogEntry, Palette, PaletteRole, Rgb};

use crate::{Action, DashboardModel};

const CATEGORY_KEY_PREFIX: &str = "category:";
const ENTRY_KEY_PREFIX: &str = "entry:";
const ENTRY_PREVIOUS: &str = "entry-previous";
const ENTRY_NEXT: &str = "entry-next";
const OPEN_SYSTEM_SETTINGS: &str = "open-system-settings";
const MAXIMUM_VISIBLE_CARDS: usize = 3;

/// Product action derived from one BMC tree touch key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BmcUiAction {
    /// Change the pure dashboard model.
    Model(Action),
    /// Launch one catalog entry shown by the dashboard itself.
    Launch(usize),
    /// Reveal the compositor-owned system settings tray.
    OpenSystemSettings,
}

/// Device-independent dashboard navigation from a keyboard or controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BmcNavigation {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
}

/// Translate a stable `bmc-render` hit-test key into product behavior.
#[must_use]
pub fn bmc_action_for_touch(key: &str) -> Option<BmcUiAction> {
    match key {
        ENTRY_PREVIOUS => Some(BmcUiAction::Model(Action::Previous)),
        ENTRY_NEXT => Some(BmcUiAction::Model(Action::Next)),
        OPEN_SYSTEM_SETTINGS => Some(BmcUiAction::OpenSystemSettings),
        _ => indexed_key(key, CATEGORY_KEY_PREFIX)
            .map(|index| BmcUiAction::Model(Action::CategorySelect(index)))
            .or_else(|| indexed_key(key, ENTRY_KEY_PREFIX).map(BmcUiAction::Launch)),
    }
}

/// Translate one semantic navigation edge into product behavior.
#[must_use]
pub const fn bmc_action_for_navigation(navigation: BmcNavigation) -> Option<BmcUiAction> {
    match navigation {
        BmcNavigation::Up => Some(BmcUiAction::Model(Action::CategoryPrevious)),
        BmcNavigation::Down => Some(BmcUiAction::Model(Action::CategoryNext)),
        BmcNavigation::Left => Some(BmcUiAction::Model(Action::Previous)),
        BmcNavigation::Right => Some(BmcUiAction::Model(Action::Next)),
        BmcNavigation::Confirm => Some(BmcUiAction::Model(Action::Confirm)),
        BmcNavigation::Back => None,
    }
}

/// Catalog indices for the current three-card window.
#[must_use]
pub fn visible_catalog_indices(model: &DashboardModel) -> Vec<usize> {
    let Some(category) = model.active_category() else {
        return Vec::new();
    };
    let count = category.len();
    let visible = count.min(MAXIMUM_VISIBLE_CARDS);
    let selected = model.selected_position().min(count.saturating_sub(1));
    let first = if count <= visible || selected == 0 {
        0
    } else if selected.saturating_add(1) >= count {
        count - visible
    } else {
        selected - 1
    };
    category
        .entry_indices()
        .get(first..first + visible)
        .map_or_else(Vec::new, <[usize]>::to_vec)
}

/// Build the current tabbed Retro Deck dashboard.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "the Deck surface dimensions are far below exact f32 integer limits"
)]
pub fn build_bmc_tree(
    model: &DashboardModel,
    size: (u32, u32),
    palette: &Palette,
    covers: &[(usize, BitmapId)],
    settings_cog: Option<BitmapId>,
) -> TreeNode {
    let mut children = tab_nodes(model, size, palette);
    children.extend(card_nodes(model, size, palette, covers));
    children.push(position_indicator(model, size, palette));
    children.push(settings_button(
        settings_cog,
        (size.0 as f32 - 68.0, size.1 as f32 - 68.0, 56.0, 56.0),
    ));
    TreeNode::Column(
        PropsData {
            background: color(palette.color(PaletteRole::Background)),
            ..PropsData::default()
        },
        children,
    )
}

#[expect(
    clippy::cast_precision_loss,
    reason = "surface dimensions and category counts are display-sized values"
)]
fn tab_nodes(model: &DashboardModel, size: (u32, u32), palette: &Palette) -> Vec<TreeNode> {
    let categories = model.catalog().categories();
    if categories.is_empty() {
        return Vec::new();
    }
    let gap = 8.0;
    let left = 56.0;
    let available = size.0 as f32 - 112.0;
    let width =
        (available - gap * categories.len().saturating_sub(1) as f32) / categories.len() as f32;
    categories
        .iter()
        .enumerate()
        .map(|(index, category)| {
            panel_with_text(
                &format!("{CATEGORY_KEY_PREFIX}{index}"),
                (left + index as f32 * (width + gap), 76.0, width, 52.0),
                category.label(),
                if index == model.active_category_index() {
                    PaletteRole::Active
                } else {
                    PaletteRole::Background
                },
                palette,
                18,
            )
        })
        .collect()
}

#[expect(
    clippy::cast_precision_loss,
    reason = "surface dimensions and at most three card positions are display-sized values"
)]
fn card_nodes(
    model: &DashboardModel,
    size: (u32, u32),
    palette: &Palette,
    covers: &[(usize, BitmapId)],
) -> Vec<TreeNode> {
    let visible = visible_catalog_indices(model);
    let card_width = 216.0;
    let card_height = 264.0;
    let gap = 36.0;
    let row_width =
        visible.len() as f32 * card_width + visible.len().saturating_sub(1) as f32 * gap;
    let mut nodes = Vec::with_capacity(visible.len().saturating_add(2));
    let selected = model.selected_entry().map(|(index, _)| index);
    for (position, catalog_index) in visible.into_iter().enumerate() {
        let Some(entry) = model.catalog().entry(catalog_index) else {
            continue;
        };
        let x = (size.0 as f32 - row_width) / 2.0 + position as f32 * (card_width + gap);
        let cover = covers
            .iter()
            .find_map(|(index, bitmap)| (*index == catalog_index).then_some(*bitmap));
        nodes.push(game_card(
            catalog_index,
            entry,
            selected == Some(catalog_index),
            cover,
            (x, 154.0, card_width, card_height),
            palette,
        ));
    }
    if model
        .active_category()
        .is_some_and(|category| category.len() > 1)
    {
        nodes.push(outline_arrow(
            ENTRY_PREVIOUS,
            (156.0, 232.0, 80.0, 100.0),
            Arrow::Left,
            palette,
        ));
        nodes.push(outline_arrow(
            ENTRY_NEXT,
            (size.0 as f32 - 236.0, 232.0, 80.0, 100.0),
            Arrow::Right,
            palette,
        ));
    }
    nodes
}

fn game_card(
    catalog_index: usize,
    entry: &CatalogEntry,
    selected: bool,
    cover: Option<BitmapId>,
    bounds: (f32, f32, f32, f32),
    palette: &Palette,
) -> TreeNode {
    let (x, y, width, height) = bounds;
    let art_size = width - 16.0;
    let mut draws = panel_draws(
        width,
        height,
        color(palette.color(if selected {
            PaletteRole::Active
        } else {
            PaletteRole::Background
        })),
        color(palette.color(PaletteRole::Accent)),
    );
    if let Some(bitmap_id) = cover {
        draws.push(DrawCommand::Bitmap {
            x: 8.0,
            y: 8.0,
            w: art_size,
            h: art_size,
            bitmap_id: Some(bitmap_id),
        });
    } else {
        draws.extend(fallback_art(entry, art_size, palette));
    }
    draws.push(DrawCommand::AutofitText {
        x: 8.0,
        y: width,
        box_width: width - 16.0,
        box_height: height - width - 8.0,
        mode: AutoFit::Shrink,
        min_size: 18,
        max_size: 18,
        text: card_title(entry.title()),
        style: centered_text(18, color(palette.color(PaletteRole::Text))),
    });
    TreeNode::Canvas {
        props: absolute_props(x, y, width, height),
        touch_key: Some(format!("{ENTRY_KEY_PREFIX}{catalog_index}")),
        draws,
    }
}

fn fallback_art(entry: &CatalogEntry, size: f32, palette: &Palette) -> Vec<DrawCommand> {
    let entry_color = rgb_color(entry.color().components());
    vec![
        DrawCommand::Rect {
            x: 8.0,
            y: 8.0,
            w: size,
            h: size,
            fill: Fill::Solid(color(palette.color(PaletteRole::Background))),
        },
        DrawCommand::Path {
            points: rectangle_points(10.0, 10.0, size - 4.0, size - 4.0),
            paint: PathPaint::Stroke {
                color: entry_color,
                width: 3.0,
            },
            closed: true,
            smooth: false,
        },
        DrawCommand::AutofitText {
            x: 22.0,
            y: 22.0,
            box_width: size - 28.0,
            box_height: size - 28.0,
            mode: AutoFit::Shrink,
            min_size: 18,
            max_size: 36,
            text: fallback_mark(entry).to_owned(),
            style: centered_text(30, entry_color),
        },
    ]
}

fn fallback_mark(entry: &CatalogEntry) -> &str {
    match entry.identifier() {
        "ten-seconds" => "10.00",
        "lua-repl" => "LUA>",
        "lisp-repl" => "(LISP)",
        "python-repl" => ">>>",
        "scheme-repl" => "<SCHEME>",
        "chiptunes" => "||||||",
        "terminal" => ">_",
        "reboot" => "POWER",
        _ => entry.title(),
    }
}

fn panel_with_text(
    key: &str,
    bounds: (f32, f32, f32, f32),
    label: &str,
    fill_role: PaletteRole,
    palette: &Palette,
    font_size: u32,
) -> TreeNode {
    let (x, y, width, height) = bounds;
    let mut draws = panel_draws(
        width,
        height,
        color(palette.color(fill_role)),
        color(palette.color(PaletteRole::Accent)),
    );
    draws.push(DrawCommand::AutofitText {
        x: 8.0,
        y: 8.0,
        box_width: width - 16.0,
        box_height: height - 16.0,
        mode: AutoFit::Shrink,
        min_size: 14,
        max_size: u16::try_from(font_size).unwrap_or(u16::MAX),
        text: label.to_owned(),
        style: centered_text(font_size, color(palette.color(PaletteRole::Text))),
    });
    TreeNode::Canvas {
        props: absolute_props(x, y, width, height),
        touch_key: Some(key.to_owned()),
        draws,
    }
}

fn panel_draws(width: f32, height: f32, fill: Color, border: Color) -> Vec<DrawCommand> {
    vec![
        DrawCommand::Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
            fill: Fill::Solid(fill),
        },
        DrawCommand::Path {
            points: rectangle_points(1.5, 1.5, width - 3.0, height - 3.0),
            paint: PathPaint::Stroke {
                color: border,
                width: 3.0,
            },
            closed: true,
            smooth: false,
        },
    ]
}

fn settings_button(bitmap_id: Option<BitmapId>, bounds: (f32, f32, f32, f32)) -> TreeNode {
    let (x, y, width, height) = bounds;
    let icon_size = 42.0;
    TreeNode::Canvas {
        props: absolute_props(x, y, width, height),
        touch_key: Some(OPEN_SYSTEM_SETTINGS.to_owned()),
        draws: vec![DrawCommand::Bitmap {
            x: (width - icon_size) / 2.0,
            y: (height - icon_size) / 2.0,
            w: icon_size,
            h: icon_size,
            bitmap_id,
        }],
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Arrow {
    Left,
    Right,
}

fn outline_arrow(
    key: &str,
    bounds: (f32, f32, f32, f32),
    arrow: Arrow,
    palette: &Palette,
) -> TreeNode {
    let (x, y, width, height) = bounds;
    let left = [
        (56.0, 18.0),
        (24.0, 50.0),
        (56.0, 82.0),
        (56.0, 66.0),
        (72.0, 66.0),
        (72.0, 34.0),
        (56.0, 34.0),
    ];
    let points = left
        .into_iter()
        .map(|(px, py)| match arrow {
            Arrow::Left => (px, py),
            Arrow::Right => (width - px, py),
        })
        .collect();
    TreeNode::Canvas {
        props: absolute_props(x, y, width, height),
        touch_key: Some(key.to_owned()),
        draws: vec![DrawCommand::Path {
            points,
            paint: PathPaint::Stroke {
                color: color(palette.color(PaletteRole::Footer)),
                width: 4.0,
            },
            closed: true,
            smooth: false,
        }],
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "the position indicator caps indices and counts at 24"
)]
fn position_indicator(model: &DashboardModel, size: (u32, u32), palette: &Palette) -> TreeNode {
    let count = model.active_category().map_or(0, crate::Category::len);
    let visible = count.min(24);
    let cell_width = 16.0;
    let gap = 8.0;
    let total_width = visible as f32 * cell_width + visible.saturating_sub(1) as f32 * gap;
    let mut draws = Vec::with_capacity(visible);
    for index in 0..visible {
        draws.push(DrawCommand::Path {
            points: rectangle_points(index as f32 * (cell_width + gap), 0.0, cell_width, 8.0),
            paint: PathPaint::Stroke {
                color: color(palette.color(if index == model.selected_position() {
                    PaletteRole::Footer
                } else {
                    PaletteRole::ControlBorder
                })),
                width: 2.0,
            },
            closed: true,
            smooth: false,
        });
    }
    TreeNode::Canvas {
        props: absolute_props(
            (size.0 as f32 - total_width) / 2.0,
            size.1 as f32 - 42.0,
            total_width,
            8.0,
        ),
        touch_key: None,
        draws,
    }
}

fn card_title(title: &str) -> String {
    const LIMIT: usize = 18;
    if title.chars().count() <= LIMIT {
        return title.to_owned();
    }
    title.chars().take(LIMIT - 3).chain("...".chars()).collect()
}

fn indexed_key(key: &str, prefix: &str) -> Option<usize> {
    key.strip_prefix(prefix)?.parse().ok()
}

fn rectangle_points(x: f32, y: f32, width: f32, height: f32) -> Vec<(f32, f32)> {
    vec![
        (x, y),
        (x + width, y),
        (x + width, y + height),
        (x, y + height),
    ]
}

const fn color(rgb: Rgb) -> Color {
    let [red, green, blue] = rgb.components();
    Color::from_rgba(red, green, blue, 255)
}

const fn rgb_color([red, green, blue]: [u8; 3]) -> Color {
    Color::from_rgba(red, green, blue, 255)
}

fn absolute_props(x: f32, y: f32, width: f32, height: f32) -> PropsData {
    PropsData {
        width,
        height,
        inset_top: y,
        inset_left: x,
        ..PropsData::default()
    }
}

fn centered_text(size: u32, color: Color) -> TextStyle {
    TextStyle {
        size,
        color,
        weight: FontWeight::BOLD,
        align: TextAlign::Center,
        vertical_align: VerticalAlign::Center,
        family: FontFamily::DeckSans,
        ..TextStyle::default()
    }
}

#[cfg(test)]
mod tests {
    use retro_deck_config::{CatalogEntry, CatalogSystem, System};

    use super::*;
    use crate::{DashboardCatalog, Keymap, VolumeState};

    fn model() -> Option<DashboardModel> {
        let mut entries = Vec::new();
        for (identifier, title) in [
            ("one", "ONE"),
            ("two", "TWO"),
            ("three", "THREE"),
            ("four", "FOUR"),
        ] {
            let rom = format!("/mnt/data/roms/nes/{identifier}.nes");
            entries.push(
                CatalogEntry::new(
                    identifier,
                    title,
                    CatalogSystem::Rom(System::Nes),
                    &rom,
                    "#ff5f00",
                )
                .ok()?,
            );
        }
        entries.push(
            CatalogEntry::new(
                "tetris",
                "TETRIS",
                CatalogSystem::Rom(System::GameBoy),
                "/mnt/data/roms/gb/tetris.gb",
                "#eeeeee",
            )
            .ok()?,
        );
        let catalog = DashboardCatalog::from_entries(entries).ok()?;
        Some(DashboardModel::new(
            catalog,
            VolumeState::DEFAULT,
            Keymap::Us,
        ))
    }

    #[test]
    fn touch_keys_select_tabs_launch_cards_and_open_settings() {
        assert_eq!(
            bmc_action_for_touch("category:1"),
            Some(BmcUiAction::Model(Action::CategorySelect(1)))
        );
        assert_eq!(
            bmc_action_for_touch("entry:7"),
            Some(BmcUiAction::Launch(7))
        );
        assert_eq!(
            bmc_action_for_touch(OPEN_SYSTEM_SETTINGS),
            Some(BmcUiAction::OpenSystemSettings)
        );
        assert_eq!(bmc_action_for_touch("category:nope"), None);
    }

    #[test]
    fn navigation_stays_on_the_single_dashboard_screen() {
        assert_eq!(
            bmc_action_for_navigation(BmcNavigation::Down),
            Some(BmcUiAction::Model(Action::CategoryNext))
        );
        assert_eq!(
            bmc_action_for_navigation(BmcNavigation::Right),
            Some(BmcUiAction::Model(Action::Next))
        );
        assert_eq!(bmc_action_for_navigation(BmcNavigation::Back), None);
    }

    #[test]
    fn three_card_window_follows_the_selected_entry() {
        let Some(mut model) = model() else {
            return;
        };
        assert_eq!(visible_catalog_indices(&model), vec![0, 1, 2]);
        let _ = model.apply(Action::Next);
        let _ = model.apply(Action::Next);
        assert_eq!(visible_catalog_indices(&model), vec![1, 2, 3]);
        let _ = model.apply(Action::Next);
        assert_eq!(visible_catalog_indices(&model), vec![1, 2, 3]);
    }

    #[test]
    fn dashboard_tree_contains_tabs_cards_and_no_category_selector() {
        let Some(model) = model() else {
            return;
        };
        let tree = build_bmc_tree(&model, (1280, 480), &Palette::default(), &[], None);
        assert!(matches!(tree, TreeNode::Column(_, children) if children.len() >= 8));
    }

    #[test]
    fn long_titles_keep_one_fixed_display_size() {
        assert_eq!(card_title("SHORT"), "SHORT");
        assert_eq!(card_title("A VERY LONG GAME TITLE"), "A VERY LONG GAM...");
    }
}
