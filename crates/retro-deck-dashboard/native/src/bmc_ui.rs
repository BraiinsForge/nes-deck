//! Retro Deck product UI expressed as a native `bmc-render` tree.

use bmc_render::tree::{
    AutoFit, Color, DrawCommand, Fill, FontFamily, FontWeight, PathPaint, PropsData, TextAlign,
    TextStyle, TreeNode, VerticalAlign,
};

use crate::{Action, Category, DashboardModel};

const BACKGROUND: Color = Color::from_rgba(0, 0, 0, 255);
const ORANGE: Color = Color::from_rgba(255, 95, 0, 255);
const WHITE: Color = Color::from_rgba(238, 238, 238, 255);
const DIM: Color = Color::from_rgba(112, 112, 112, 255);
const PANEL: Color = Color::from_rgba(24, 24, 24, 255);

const CATEGORY_PREVIOUS: &str = "category-previous";
const CATEGORY_NEXT: &str = "category-next";
const OPEN_CATEGORY: &str = "open-category";
const ENTRY_PREVIOUS: &str = "entry-previous";
const ENTRY_NEXT: &str = "entry-next";
const OPEN_ENTRY: &str = "open-entry";
const CLOSE_CAROUSEL: &str = "close-carousel";

/// Which of the two approved dashboard views is visible.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BmcScreen {
    /// Large centered console/category selector.
    #[default]
    Categories,
    /// Selected game/application carousel.
    Carousel,
}

/// Product action derived from one BMC tree touch key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BmcUiAction {
    /// Change the pure dashboard model.
    Model(Action),
    /// Enter the selected category's carousel.
    OpenCarousel,
    /// Return to the category selector.
    CloseCarousel,
}

/// Translate a stable `bmc-render` hit-test key into product behavior.
#[must_use]
pub fn bmc_action_for_touch(screen: BmcScreen, key: &str) -> Option<BmcUiAction> {
    match (screen, key) {
        (BmcScreen::Categories, CATEGORY_PREVIOUS) => {
            Some(BmcUiAction::Model(Action::CategoryPrevious))
        }
        (BmcScreen::Categories, CATEGORY_NEXT) => Some(BmcUiAction::Model(Action::CategoryNext)),
        (BmcScreen::Categories, OPEN_CATEGORY) => Some(BmcUiAction::OpenCarousel),
        (BmcScreen::Carousel, ENTRY_PREVIOUS) => Some(BmcUiAction::Model(Action::Previous)),
        (BmcScreen::Carousel, ENTRY_NEXT) => Some(BmcUiAction::Model(Action::Next)),
        (BmcScreen::Carousel, OPEN_ENTRY) => Some(BmcUiAction::Model(Action::Confirm)),
        (BmcScreen::Carousel, CLOSE_CAROUSEL) => Some(BmcUiAction::CloseCarousel),
        _ => None,
    }
}

/// Build the current Retro Deck screen with BMC-native layout and drawing.
#[must_use]
pub fn build_bmc_tree(model: &DashboardModel, screen: BmcScreen, size: (u32, u32)) -> TreeNode {
    let children = match screen {
        BmcScreen::Categories => category_nodes(model, size),
        BmcScreen::Carousel => carousel_nodes(model, size),
    };
    TreeNode::Column(
        PropsData {
            background: BACKGROUND,
            ..PropsData::default()
        },
        children,
    )
}

#[expect(
    clippy::cast_precision_loss,
    reason = "surface dimensions are small integers represented by the renderer as f32"
)]
fn category_nodes(model: &DashboardModel, size: (u32, u32)) -> Vec<TreeNode> {
    let width = size.0 as f32;
    let height = size.1 as f32;
    let button_w = (width * 0.38).clamp(360.0, 520.0);
    let button_h = (height * 0.38).clamp(150.0, 196.0);
    let button_x = (width - button_w) / 2.0 - 36.0;
    let button_y = (height - button_h) / 2.0;
    let arrow_size = 72.0;
    let arrow_x = button_x + button_w + 28.0;
    let label = model
        .active_category()
        .map_or("---", |category| category.label());

    vec![
        text_button(
            OPEN_CATEGORY,
            (button_x, button_y, button_w, button_h),
            label,
            ORANGE,
            BACKGROUND,
            42,
        ),
        arrow_button(
            CATEGORY_PREVIOUS,
            (arrow_x, button_y, arrow_size, arrow_size),
            Arrow::Up,
        ),
        arrow_button(
            CATEGORY_NEXT,
            (
                arrow_x,
                button_y + button_h - arrow_size,
                arrow_size,
                arrow_size,
            ),
            Arrow::Down,
        ),
    ]
}

#[expect(
    clippy::cast_precision_loss,
    reason = "surface dimensions are small integers represented by the renderer as f32"
)]
fn carousel_nodes(model: &DashboardModel, size: (u32, u32)) -> Vec<TreeNode> {
    let width = size.0 as f32;
    let height = size.1 as f32;
    let card_w = (width * 0.36).clamp(380.0, 500.0);
    let card_h = (height * 0.58).clamp(240.0, 300.0);
    let card_x = (width - card_w) / 2.0;
    let card_y = 72.0;
    let arrow_size = 80.0;
    let title = model
        .selected_entry()
        .map_or("NO ENTRY", |(_, entry)| entry.title());
    let count = model.active_category().map_or(0, Category::len);

    vec![
        arrow_button(CLOSE_CAROUSEL, (24.0, 18.0, 56.0, 56.0), Arrow::Close),
        arrow_button(
            ENTRY_PREVIOUS,
            (
                card_x - arrow_size - 44.0,
                card_y + (card_h - arrow_size) / 2.0,
                arrow_size,
                arrow_size,
            ),
            Arrow::Left,
        ),
        text_button(
            OPEN_ENTRY,
            (card_x, card_y, card_w, card_h),
            title,
            PANEL,
            WHITE,
            30,
        ),
        arrow_button(
            ENTRY_NEXT,
            (
                card_x + card_w + 44.0,
                card_y + (card_h - arrow_size) / 2.0,
                arrow_size,
                arrow_size,
            ),
            Arrow::Right,
        ),
        position_indicator(
            model.selected_position(),
            count,
            (width / 2.0, height - 42.0),
        ),
    ]
}

fn text_button(
    key: &str,
    bounds: (f32, f32, f32, f32),
    label: &str,
    fill: Color,
    text_color: Color,
    font_size: u32,
) -> TreeNode {
    let (x, y, width, height) = bounds;
    TreeNode::Canvas {
        props: absolute_props(x, y, width, height),
        touch_key: Some(key.to_owned()),
        draws: vec![
            DrawCommand::Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
                fill: Fill::Solid(fill),
            },
            DrawCommand::AutofitText {
                x: 24.0,
                y: 18.0,
                box_width: width - 48.0,
                box_height: height - 36.0,
                mode: AutoFit::Shrink,
                min_size: 18,
                max_size: u16::try_from(font_size).unwrap_or(u16::MAX),
                text: label.to_owned(),
                style: centered_text(font_size, text_color),
            },
        ],
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Arrow {
    Up,
    Down,
    Left,
    Right,
    Close,
}

fn arrow_button(key: &str, bounds: (f32, f32, f32, f32), arrow: Arrow) -> TreeNode {
    let (x, y, width, height) = bounds;
    let inset = width.min(height) * 0.2;
    let points = match arrow {
        Arrow::Up => vec![
            (width / 2.0, inset),
            (width - inset, height - inset),
            (inset, height - inset),
        ],
        Arrow::Down => vec![
            (inset, inset),
            (width - inset, inset),
            (width / 2.0, height - inset),
        ],
        Arrow::Left => vec![
            (inset, height / 2.0),
            (width - inset, inset),
            (width - inset, height - inset),
        ],
        Arrow::Right => vec![
            (inset, inset),
            (width - inset, height / 2.0),
            (inset, height - inset),
        ],
        Arrow::Close => return close_button(key, bounds),
    };
    TreeNode::Canvas {
        props: absolute_props(x, y, width, height),
        touch_key: Some(key.to_owned()),
        draws: vec![DrawCommand::Path {
            points,
            paint: PathPaint::Fill(Fill::Solid(ORANGE)),
            closed: true,
            smooth: false,
        }],
    }
}

fn close_button(key: &str, bounds: (f32, f32, f32, f32)) -> TreeNode {
    let (x, y, width, height) = bounds;
    let inset = 16.0;
    TreeNode::Canvas {
        props: absolute_props(x, y, width, height),
        touch_key: Some(key.to_owned()),
        draws: vec![
            DrawCommand::Path {
                points: vec![(inset, inset), (width - inset, height - inset)],
                paint: PathPaint::Stroke {
                    color: WHITE,
                    width: 4.0,
                },
                closed: false,
                smooth: false,
            },
            DrawCommand::Path {
                points: vec![(width - inset, inset), (inset, height - inset)],
                paint: PathPaint::Stroke {
                    color: WHITE,
                    width: 4.0,
                },
                closed: false,
                smooth: false,
            },
        ],
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "the indicator caps indices and counts at 24"
)]
fn position_indicator(selected: usize, count: usize, center: (f32, f32)) -> TreeNode {
    let visible = count.min(24);
    let cell_w = 12.0;
    let gap = 5.0;
    let total_w = visible as f32 * cell_w + visible.saturating_sub(1) as f32 * gap;
    let mut draws = Vec::with_capacity(visible);
    for index in 0..visible {
        draws.push(DrawCommand::Rect {
            x: index as f32 * (cell_w + gap),
            y: 0.0,
            w: cell_w,
            h: 5.0,
            fill: Fill::Solid(if index == selected { ORANGE } else { DIM }),
        });
    }
    TreeNode::Canvas {
        props: absolute_props(center.0 - total_w / 2.0, center.1, total_w, 5.0),
        touch_key: None,
        draws,
    }
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
    use crate::{Brightness, DashboardCatalog, Keymap, VolumeState};

    fn model() -> Option<DashboardModel> {
        let entries = [
            CatalogEntry::new(
                "mario",
                "MARIO BROS.",
                CatalogSystem::Rom(System::Nes),
                "/mnt/data/roms/nes/mario.nes",
                "#ff5f00",
            )
            .ok()?,
            CatalogEntry::new(
                "tetris",
                "TETRIS",
                CatalogSystem::Rom(System::GameBoy),
                "/mnt/data/roms/gb/tetris.gb",
                "#eeeeee",
            )
            .ok()?,
        ];
        let catalog = DashboardCatalog::from_entries(entries).ok()?;
        Some(DashboardModel::new(
            catalog,
            VolumeState::DEFAULT,
            Brightness::DEFAULT,
            Keymap::Us,
        ))
    }

    #[test]
    fn category_touch_keys_map_only_on_category_screen() {
        assert_eq!(
            bmc_action_for_touch(BmcScreen::Categories, CATEGORY_NEXT),
            Some(BmcUiAction::Model(Action::CategoryNext))
        );
        assert_eq!(
            bmc_action_for_touch(BmcScreen::Carousel, CATEGORY_NEXT),
            None
        );
    }

    #[test]
    fn carousel_touch_keys_map_only_on_carousel_screen() {
        assert_eq!(
            bmc_action_for_touch(BmcScreen::Carousel, OPEN_ENTRY),
            Some(BmcUiAction::Model(Action::Confirm))
        );
        assert_eq!(
            bmc_action_for_touch(BmcScreen::Categories, OPEN_ENTRY),
            None
        );
    }

    #[test]
    fn each_screen_builds_from_the_existing_product_model() {
        let Some(model) = model() else {
            return;
        };
        assert!(matches!(
            build_bmc_tree(&model, BmcScreen::Categories, (1280, 480)),
            TreeNode::Column(_, children) if children.len() == 3
        ));
        assert!(matches!(
            build_bmc_tree(&model, BmcScreen::Carousel, (1280, 480)),
            TreeNode::Column(_, children) if children.len() == 5
        ));
    }
}
