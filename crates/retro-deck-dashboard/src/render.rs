//! Deterministic RGB565 rendering for the dashboard catalog screen.

use std::{fmt, fmt::Write as _};

use retro_deck_config::{CatalogEntry, CatalogSystem, Palette, PaletteRole, Rgb};
use retro_deck_ui::{Canvas, Rect, TextBuffer, fit_text_scale, rgb888_to_rgb565};

use crate::credits::{CreditsCrawl, CreditsLayout, draw_credits};
use crate::settings::{SettingsLayout, SettingsView, draw_settings};
use crate::{Action, DashboardModel, Keymap, Status};

/// Logical dashboard width presented to the platform layer.
pub const CANVAS_WIDTH: usize = 1_280;
/// Logical dashboard height presented to the platform layer.
pub const CANVAS_HEIGHT: usize = 480;
const PIXELS: usize = CANVAS_WIDTH * CANVAS_HEIGHT;
const MAXIMUM_CATEGORIES: usize = 6;
const MAXIMUM_VISIBLE_ENTRIES: usize = 3;
const MAXIMUM_COVER_DIMENSION: usize = 2_048;
const GAME_TITLE_SCALE: usize = 2;
const PIXEL_STROKE: usize = 4;

const SETTINGS_GEAR_SIZE: usize = 23;
const SETTINGS_GEAR: [&str; SETTINGS_GEAR_SIZE] = [
    "..........000..........",
    ".........03320.........",
    "...000...03220...000...",
    "..0333000222220003220..",
    "..0322222222222232220..",
    "..0322222222222222210..",
    "...03222100000222210...",
    "...0222100.1.0022220...",
    "...022100..1..002220...",
    ".0022200...1...0022200.",
    "0332220...010...0222220",
    "03222201111011110222220",
    "0222220...010...0222210",
    ".0022200...1...0022200.",
    "...022200..1..003220...",
    "...0222200.1.0032220...",
    "...03222200000322220...",
    "..0322222222222222220..",
    "..0222222222222222220..",
    "..0221000222220003110..",
    "...000...02220...000...",
    ".........02210.........",
    "..........000..........",
];

/// One validated borrowed RGB565 cover image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cover<'pixels> {
    width: usize,
    height: usize,
    pixels: &'pixels [u16],
}

impl<'pixels> Cover<'pixels> {
    /// Validate dimensions and exact tightly packed storage.
    ///
    /// # Errors
    ///
    /// Returns [`CoverError`] for zero, excessive, overflowing, or mismatched
    /// cover geometry.
    pub fn new(width: usize, height: usize, pixels: &'pixels [u16]) -> Result<Self, CoverError> {
        if width == 0
            || height == 0
            || width > MAXIMUM_COVER_DIMENSION
            || height > MAXIMUM_COVER_DIMENSION
        {
            return Err(CoverError::Dimensions);
        }
        if width.checked_mul(height) != Some(pixels.len()) {
            return Err(CoverError::Storage);
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    fn pixel(self, x: usize, y: usize) -> Option<u16> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels
            .get(y.checked_mul(self.width)?.checked_add(x)?)
            .copied()
    }
}

/// Invalid decoded cover geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverError {
    /// A dimension is zero or exceeds the fixed decoder contract.
    Dimensions,
    /// Pixel storage is not exactly width times height.
    Storage,
}

impl fmt::Display for CoverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dimensions => formatter.write_str("cover dimensions must be in 1 through 2048"),
            Self::Storage => formatter.write_str("cover pixel storage does not match its geometry"),
        }
    }
}

impl std::error::Error for CoverError {}

/// Read-only artwork lookup kept outside catalog and navigation state.
pub trait ArtworkProvider {
    /// Return prevalidated RGB565 artwork for one stable catalog identifier.
    fn cover(&self, identifier: &str) -> Option<Cover<'_>>;
}

/// Artwork provider used when the optional persistent cache is empty.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoArtwork;

impl ArtworkProvider for NoArtwork {
    fn cover(&self, _identifier: &str) -> Option<Cover<'_>> {
        None
    }
}

/// One rendered and touchable game card.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryButton {
    bounds: Rect,
    entry_index: usize,
}

impl EntryButton {
    /// Card bounds in logical dashboard coordinates.
    #[must_use]
    pub const fn bounds(self) -> Rect {
        self.bounds
    }

    /// Stable index into the owning dashboard catalog.
    #[must_use]
    pub const fn entry_index(self) -> usize {
        self.entry_index
    }
}

/// Fixed-capacity hit-test result of one catalog render.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuLayout {
    credits_button: Rect,
    settings_button: Rect,
    previous_button: Option<Rect>,
    next_button: Option<Rect>,
    category_buttons: [Option<Rect>; MAXIMUM_CATEGORIES],
    entry_buttons: [Option<EntryButton>; MAXIMUM_VISIBLE_ENTRIES],
    selected_entry_index: Option<usize>,
}

impl MenuLayout {
    const fn empty() -> Self {
        Self {
            credits_button: Rect::new(12, 412, 56, 56),
            settings_button: Rect::new(1_212, 412, 56, 56),
            previous_button: None,
            next_button: None,
            category_buttons: [None; MAXIMUM_CATEGORIES],
            entry_buttons: [None; MAXIMUM_VISIBLE_ENTRIES],
            selected_entry_index: None,
        }
    }

    /// Bottom-left credits control.
    #[must_use]
    pub const fn credits_button(&self) -> Rect {
        self.credits_button
    }

    /// Bottom-right settings control.
    #[must_use]
    pub const fn settings_button(&self) -> Rect {
        self.settings_button
    }

    /// Previous-game arrow when the active category has multiple entries.
    #[must_use]
    pub const fn previous_button(&self) -> Option<Rect> {
        self.previous_button
    }

    /// Next-game arrow when the active category has multiple entries.
    #[must_use]
    pub const fn next_button(&self) -> Option<Rect> {
        self.next_button
    }

    /// Category controls in stable catalog order.
    #[must_use]
    pub const fn category_buttons(&self) -> &[Option<Rect>; MAXIMUM_CATEGORIES] {
        &self.category_buttons
    }

    /// Up to three visible catalog cards.
    #[must_use]
    pub const fn entry_buttons(&self) -> &[Option<EntryButton>; MAXIMUM_VISIBLE_ENTRIES] {
        &self.entry_buttons
    }

    /// Selected owning-catalog index.
    #[must_use]
    pub const fn selected_entry_index(&self) -> Option<usize> {
        self.selected_entry_index
    }

    /// Map one committed touch coordinate to a semantic model action.
    #[must_use]
    pub fn action_at(&self, x: usize, y: usize) -> Option<Action> {
        if self.credits_button.contains(x, y) {
            return Some(Action::ShowCredits);
        }
        if self.settings_button.contains(x, y) {
            return Some(Action::ToggleSettings);
        }
        if self
            .previous_button
            .is_some_and(|bounds| bounds.contains(x, y))
        {
            return Some(Action::Previous);
        }
        if self.next_button.is_some_and(|bounds| bounds.contains(x, y)) {
            return Some(Action::Next);
        }
        for (index, bounds) in self.category_buttons.iter().enumerate() {
            if bounds.is_some_and(|bounds| bounds.contains(x, y)) {
                return Some(Action::SelectCategory(index));
            }
        }
        self.entry_buttons.iter().flatten().find_map(|button| {
            button
                .bounds
                .contains(x, y)
                .then_some(Action::ActivateEntry(button.entry_index))
        })
    }
}

/// Screen represented by the current frame pixels and hit geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderedScreen {
    /// Console tabs and game cards.
    Menu,
    /// Device and application settings controls.
    Settings,
    /// License and attribution crawl.
    Credits,
}

/// Fixed-size dashboard frame plus exact hit-test geometry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardFrame {
    pixels: Vec<u16>,
    screen: RenderedScreen,
    menu_layout: MenuLayout,
}

impl DashboardFrame {
    /// Allocate and render one dashboard catalog frame without optional art.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] if the fixed frame allocation fails.
    pub fn render_menu(model: &DashboardModel, palette: &Palette) -> Result<Self, RenderError> {
        Self::render_menu_with_artwork(model, palette, &NoArtwork)
    }

    /// Allocate and render one dashboard catalog frame with optional art.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] if the fixed frame allocation fails.
    pub fn render_menu_with_artwork<P: ArtworkProvider + ?Sized>(
        model: &DashboardModel,
        palette: &Palette,
        artwork: &P,
    ) -> Result<Self, RenderError> {
        let mut frame = Self::allocate(palette)?;
        frame.redraw_menu_with_artwork(model, palette, artwork);
        Ok(frame)
    }

    /// Allocate and render device settings from read-only status.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] if the fixed frame allocation fails.
    pub fn render_settings(
        model: &DashboardModel,
        palette: &Palette,
        view: SettingsView<'_>,
    ) -> Result<Self, RenderError> {
        let mut frame = Self::allocate(palette)?;
        frame.redraw_settings(model, palette, view);
        Ok(frame)
    }

    /// Allocate and render software credits.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] if the fixed frame allocation fails.
    pub fn render_credits(
        crawl: &CreditsCrawl,
        palette: &Palette,
        reduced_motion: bool,
        elapsed_ms: u64,
    ) -> Result<Self, RenderError> {
        let mut frame = Self::allocate(palette)?;
        frame.redraw_credits(crawl, palette, reduced_motion, elapsed_ms);
        Ok(frame)
    }

    /// Redraw in the existing allocation without optional artwork.
    pub fn redraw_menu(&mut self, model: &DashboardModel, palette: &Palette) {
        self.redraw_menu_with_artwork(model, palette, &NoArtwork);
    }

    /// Redraw in the existing allocation with a read-only artwork source.
    pub fn redraw_menu_with_artwork<P: ArtworkProvider + ?Sized>(
        &mut self,
        model: &DashboardModel,
        palette: &Palette,
        artwork: &P,
    ) {
        let Some(mut canvas) = Canvas::new(&mut self.pixels, CANVAS_WIDTH, CANVAS_HEIGHT) else {
            return;
        };
        self.menu_layout = draw_menu(&mut canvas, model, palette, artwork);
        self.screen = RenderedScreen::Menu;
    }

    /// Redraw settings in the existing allocation from a read-only view.
    pub fn redraw_settings(
        &mut self,
        model: &DashboardModel,
        palette: &Palette,
        view: SettingsView<'_>,
    ) {
        let Some(mut canvas) = Canvas::new(&mut self.pixels, CANVAS_WIDTH, CANVAS_HEIGHT) else {
            return;
        };
        let _ = draw_settings(&mut canvas, model, palette, view);
        self.screen = RenderedScreen::Settings;
    }

    /// Redraw software credits in the existing frame allocation.
    pub fn redraw_credits(
        &mut self,
        crawl: &CreditsCrawl,
        palette: &Palette,
        reduced_motion: bool,
        elapsed_ms: u64,
    ) {
        let Some(mut canvas) = Canvas::new(&mut self.pixels, CANVAS_WIDTH, CANVAS_HEIGHT) else {
            return;
        };
        let _ = draw_credits(&mut canvas, crawl, palette, reduced_motion, elapsed_ms);
        self.screen = RenderedScreen::Credits;
    }

    /// Borrow tightly packed native-endian RGB565 pixels.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref Vec to a slice"
    )]
    pub fn pixels(&self) -> &[u16] {
        &self.pixels
    }

    /// Fixed row stride in bytes.
    #[must_use]
    pub const fn stride_bytes() -> usize {
        CANVAS_WIDTH * size_of::<u16>()
    }

    /// Screen currently represented by the pixel allocation.
    #[must_use]
    pub const fn rendered_screen(&self) -> RenderedScreen {
        self.screen
    }

    /// Menu geometry when the current frame contains the catalog screen.
    #[must_use]
    pub const fn menu_layout(&self) -> Option<&MenuLayout> {
        match self.screen {
            RenderedScreen::Menu => Some(&self.menu_layout),
            RenderedScreen::Settings | RenderedScreen::Credits => None,
        }
    }

    /// Settings geometry when the current frame contains settings.
    #[must_use]
    pub const fn settings_layout(&self) -> Option<SettingsLayout> {
        match self.screen {
            RenderedScreen::Settings => Some(SettingsLayout::fixed()),
            RenderedScreen::Menu | RenderedScreen::Credits => None,
        }
    }

    /// Credits geometry when the current frame contains attributions.
    #[must_use]
    pub const fn credits_layout(&self) -> Option<CreditsLayout> {
        match self.screen {
            RenderedScreen::Credits => Some(CreditsLayout::fixed()),
            RenderedScreen::Menu | RenderedScreen::Settings => None,
        }
    }

    /// Map one coordinate on the represented complete frame to an action.
    #[must_use]
    pub fn action_at(&self, x: usize, y: usize) -> Option<Action> {
        match self.screen {
            RenderedScreen::Menu => self.menu_layout.action_at(x, y),
            RenderedScreen::Settings => SettingsLayout::fixed().action_at(x, y),
            RenderedScreen::Credits => CreditsLayout::fixed().action_at(x, y),
        }
    }

    /// Read one pixel for screenshots and regression tests.
    #[must_use]
    pub fn pixel(&self, x: usize, y: usize) -> Option<u16> {
        if x >= CANVAS_WIDTH || y >= CANVAS_HEIGHT {
            return None;
        }
        self.pixels
            .get(y.checked_mul(CANVAS_WIDTH)?.checked_add(x)?)
            .copied()
    }

    fn allocate(palette: &Palette) -> Result<Self, RenderError> {
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(PIXELS).map_err(|_| RenderError)?;
        pixels.resize(PIXELS, palette_pixel(palette, PaletteRole::Background));
        Ok(Self {
            pixels,
            screen: RenderedScreen::Menu,
            menu_layout: MenuLayout::empty(),
        })
    }
}

/// Fixed dashboard frame allocation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderError;

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("cannot allocate the dashboard frame")
    }
}

impl std::error::Error for RenderError {}

fn draw_menu<P: ArtworkProvider + ?Sized>(
    canvas: &mut Canvas<'_>,
    model: &DashboardModel,
    palette: &Palette,
    artwork: &P,
) -> MenuLayout {
    canvas.clear(palette_pixel(palette, PaletteRole::Background));
    let mut layout = MenuLayout::empty();
    canvas.draw_centered_text(
        layout.credits_button,
        "(c)",
        2,
        palette_pixel(palette, PaletteRole::Footer),
    );
    draw_settings_gear(canvas, layout.settings_button);

    draw_category_tabs(canvas, model, palette, &mut layout);
    draw_active_carousel(canvas, model, palette, artwork, &mut layout);
    draw_status(canvas, model.status(), palette);
    layout
}

fn draw_category_tabs(
    canvas: &mut Canvas<'_>,
    model: &DashboardModel,
    palette: &Palette,
    layout: &mut MenuLayout,
) {
    let categories = model.catalog().categories();
    let category_count = categories.len().min(MAXIMUM_CATEGORIES);
    if category_count == 0 {
        return;
    }
    let gap = 8_usize;
    let left = 56_usize;
    let width = (1_168_usize.saturating_sub(gap.saturating_mul(category_count.saturating_sub(1))))
        / category_count;
    for (index, category) in categories.iter().take(category_count).enumerate() {
        let tab = Rect::new(
            left.saturating_add(index.saturating_mul(width.saturating_add(gap))),
            76,
            width,
            52,
        );
        if let Some(slot) = layout.category_buttons.get_mut(index) {
            *slot = Some(tab);
        }
        let active = index == model.active_category_index();
        canvas.draw_panel(
            tab,
            palette_pixel(
                palette,
                if active {
                    PaletteRole::Active
                } else {
                    PaletteRole::Background
                },
            ),
            palette_pixel(palette, PaletteRole::Accent),
            PIXEL_STROKE,
        );
        canvas.draw_centered_text(
            tab,
            category.label(),
            fit_text_scale(category.label(), tab.width.saturating_sub(16), 2, 1),
            palette_pixel(palette, PaletteRole::Text),
        );
    }
}

fn draw_active_carousel<P: ArtworkProvider + ?Sized>(
    canvas: &mut Canvas<'_>,
    model: &DashboardModel,
    palette: &Palette,
    artwork: &P,
    layout: &mut MenuLayout,
) {
    let Some(category) = model.active_category() else {
        return;
    };
    let indices = category.entry_indices();
    if indices.is_empty() {
        return;
    }
    let selected_position = model.selected_position() % indices.len();
    layout.selected_entry_index = indices.get(selected_position).copied();
    let visible_count = indices.len().min(MAXIMUM_VISIBLE_ENTRIES);
    let first_position = visible_window_start(selected_position, indices.len(), visible_count);
    let card_width = 216_usize;
    let card_height = 264_usize;
    let card_gap = 36_usize;
    let row_width = visible_count
        .saturating_mul(card_width)
        .saturating_add(visible_count.saturating_sub(1).saturating_mul(card_gap));
    let mut card_x = CANVAS_WIDTH.saturating_sub(row_width) / 2;
    for visible in 0..visible_count {
        let Some(entry_index) = indices.get(first_position.saturating_add(visible)).copied() else {
            continue;
        };
        let Some(entry) = model.catalog().entry(entry_index) else {
            continue;
        };
        let card = Rect::new(card_x, 154, card_width, card_height);
        if let Some(slot) = layout.entry_buttons.get_mut(visible) {
            *slot = Some(EntryButton {
                bounds: card,
                entry_index,
            });
        }
        draw_game_card(
            canvas,
            card,
            entry,
            layout.selected_entry_index == Some(entry_index),
            palette,
            artwork.cover(entry.identifier()),
        );
        card_x = card_x.saturating_add(card_width.saturating_add(card_gap));
    }

    if indices.len() > 1 {
        let previous = Rect::new(156, 232, 80, 100);
        let next = Rect::new(1_044, 232, 80, 100);
        layout.previous_button = Some(previous);
        layout.next_button = Some(next);
        let color = palette_pixel(palette, PaletteRole::Footer);
        draw_outline_arrow(canvas, previous, ArrowDirection::Left, color);
        draw_outline_arrow(canvas, next, ArrowDirection::Right, color);
    }

    draw_position_indicators(canvas, indices.len(), selected_position, palette);
}

const fn visible_window_start(selected: usize, total: usize, visible: usize) -> usize {
    if total <= visible || selected == 0 {
        0
    } else if selected.saturating_add(1) >= total {
        total.saturating_sub(visible)
    } else {
        selected.saturating_sub(1)
    }
}

fn draw_game_card(
    canvas: &mut Canvas<'_>,
    card: Rect,
    entry: &CatalogEntry,
    selected: bool,
    palette: &Palette,
    cover: Option<Cover<'_>>,
) {
    canvas.draw_panel(
        card,
        palette_pixel(
            palette,
            if selected {
                PaletteRole::Active
            } else {
                PaletteRole::Background
            },
        ),
        palette_pixel(palette, PaletteRole::Accent),
        PIXEL_STROKE,
    );
    let art = Rect::new(
        card.x.saturating_add(8),
        card.y.saturating_add(8),
        card.width.saturating_sub(16),
        card.width.saturating_sub(16),
    );
    if let Some(cover) = cover {
        draw_cover_square(canvas, art, cover);
    } else if !draw_compact_deck_logo(canvas, art, entry, palette) {
        draw_compact_cartridge(canvas, art, game_pixel(entry), palette);
    }
    let label = Rect::new(
        card.x.saturating_add(8),
        card.y.saturating_add(card.width),
        card.width.saturating_sub(16),
        card.height.saturating_sub(card.width).saturating_sub(8),
    );
    let mut title = TextBuffer::<64>::from_display(entry.title());
    title.fit_width(label.width.saturating_sub(12), GAME_TITLE_SCALE);
    canvas.draw_centered_text(
        label,
        title.as_str(),
        GAME_TITLE_SCALE,
        palette_pixel(palette, PaletteRole::Text),
    );
}

fn draw_cover_square(canvas: &mut Canvas<'_>, bounds: Rect, cover: Cover<'_>) {
    if bounds.width == 0 || bounds.height == 0 {
        return;
    }
    let source_size = cover.width.min(cover.height);
    let source_left = cover.width.saturating_sub(source_size) / 2;
    let source_top = cover.height.saturating_sub(source_size) / 2;
    for y in 0..bounds.height {
        let source_y = source_top.saturating_add(y.saturating_mul(source_size) / bounds.height);
        for x in 0..bounds.width {
            let source_x = source_left.saturating_add(x.saturating_mul(source_size) / bounds.width);
            if let Some(pixel) = cover.pixel(source_x, source_y) {
                canvas.set_pixel(
                    bounds.x.saturating_add(x),
                    bounds.y.saturating_add(y),
                    pixel,
                );
            }
        }
    }
}

fn draw_compact_cartridge(canvas: &mut Canvas<'_>, bounds: Rect, color: u16, palette: &Palette) {
    let cartridge = Rect::new(
        bounds.x.saturating_add(34),
        bounds.y.saturating_add(28),
        bounds.width.saturating_sub(68),
        bounds.height.saturating_sub(56),
    );
    canvas.draw_panel(
        cartridge,
        palette_pixel(palette, PaletteRole::Background),
        color,
        PIXEL_STROKE,
    );
    canvas.fill_rect(
        Rect::new(
            cartridge.x.saturating_add(24),
            cartridge.y.saturating_add(26),
            cartridge.width.saturating_sub(48),
            8,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            cartridge.x.saturating_add(24),
            cartridge.y.saturating_add(46),
            cartridge.width.saturating_sub(48),
            4,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            cartridge.x.saturating_add(20),
            cartridge
                .y
                .saturating_add(cartridge.height.saturating_sub(30)),
            cartridge.width.saturating_sub(40),
            10,
        ),
        color,
    );
}

fn draw_compact_deck_logo(
    canvas: &mut Canvas<'_>,
    bounds: Rect,
    entry: &CatalogEntry,
    palette: &Palette,
) -> bool {
    if entry.system() != CatalogSystem::Deck {
        return false;
    }
    let accent = game_pixel(entry);
    match entry.identifier() {
        "ten-seconds" => canvas.draw_centered_text(bounds, "10.00", 5, accent),
        "lua-repl" => {
            canvas.draw_panel(
                Rect::new(
                    bounds.x.saturating_add(24),
                    bounds.y.saturating_add(46),
                    bounds.width.saturating_sub(48),
                    bounds.height.saturating_sub(92),
                ),
                palette_pixel(palette, PaletteRole::Background),
                accent,
                PIXEL_STROKE,
            );
            canvas.draw_centered_text(bounds, "LUA>", 4, accent);
        }
        "lisp-repl" => canvas.draw_centered_text(bounds, "(LISP)", 4, accent),
        "python-repl" => {
            canvas.draw_centered_text(bounds, ">>>", 6, accent);
            canvas.fill_rect(
                Rect::new(
                    bounds.x.saturating_add(bounds.width / 2).saturating_sub(54),
                    bounds
                        .y
                        .saturating_add(bounds.height / 2)
                        .saturating_add(34),
                    108,
                    6,
                ),
                accent,
            );
        }
        "scheme-repl" => {
            canvas.draw_centered_text(bounds, "(SCHEME)", 3, accent);
            canvas.fill_rect(
                Rect::new(
                    bounds.x.saturating_add(bounds.width / 2).saturating_sub(34),
                    bounds
                        .y
                        .saturating_add(bounds.height / 2)
                        .saturating_add(30),
                    68,
                    5,
                ),
                accent,
            );
        }
        "chiptunes" => draw_chiptune_logo(canvas, bounds, accent),
        "terminal" => draw_terminal_logo(canvas, bounds, accent, palette),
        "reboot" => draw_power_logo(canvas, bounds, accent),
        _ => return false,
    }
    true
}

fn draw_chiptune_logo(canvas: &mut Canvas<'_>, bounds: Rect, color: u16) {
    let center_x = bounds.x.saturating_add(bounds.width / 2);
    let center_y = bounds.y.saturating_add(bounds.height / 2);
    let heights = [34, 62, 92, 48, 112, 74, 42, 86, 56];
    for (index, height) in heights.into_iter().enumerate() {
        canvas.fill_rect(
            Rect::new(
                center_x
                    .saturating_sub(86)
                    .saturating_add(index.saturating_mul(20)),
                center_y.saturating_sub(height / 2),
                10,
                height,
            ),
            color,
        );
    }
}

fn draw_terminal_logo(canvas: &mut Canvas<'_>, bounds: Rect, accent: u16, palette: &Palette) {
    let screen = Rect::new(
        bounds.x.saturating_add(30),
        bounds.y.saturating_add(44),
        bounds.width.saturating_sub(60),
        96,
    );
    canvas.stroke_rect(screen, 4, accent);
    canvas.draw_centered_text(screen, ">_", 5, palette_pixel(palette, PaletteRole::Text));
    canvas.fill_rect(
        Rect::new(
            bounds.x.saturating_add(bounds.width / 2).saturating_sub(6),
            screen.y.saturating_add(screen.height),
            12,
            18,
        ),
        accent,
    );
    canvas.fill_rect(
        Rect::new(
            bounds.x.saturating_add(bounds.width / 2).saturating_sub(44),
            screen.y.saturating_add(screen.height).saturating_add(18),
            88,
            4,
        ),
        accent,
    );
}

fn draw_power_logo(canvas: &mut Canvas<'_>, bounds: Rect, color: u16) {
    let center_x = bounds.x.saturating_add(bounds.width / 2);
    let center_y = bounds.y.saturating_add(bounds.height / 2);
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_sub(5),
            center_y.saturating_sub(58),
            10,
            54,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_sub(48),
            center_y.saturating_sub(34),
            22,
            8,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_add(26),
            center_y.saturating_sub(34),
            22,
            8,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_sub(58),
            center_y.saturating_sub(26),
            8,
            54,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_add(50),
            center_y.saturating_sub(26),
            8,
            54,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_sub(48),
            center_y.saturating_add(28),
            16,
            8,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_add(32),
            center_y.saturating_add(28),
            16,
            8,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_sub(32),
            center_y.saturating_add(36),
            64,
            8,
        ),
        color,
    );
}

fn draw_position_indicators(
    canvas: &mut Canvas<'_>,
    count: usize,
    selected: usize,
    palette: &Palette,
) {
    let width = 16_usize;
    let height = 8_usize;
    let gap = 8_usize;
    let row_width = count
        .saturating_mul(width)
        .saturating_add(count.saturating_sub(1).saturating_mul(gap));
    let mut x = CANVAS_WIDTH.saturating_sub(row_width) / 2;
    for position in 0..count {
        canvas.stroke_rect(
            Rect::new(x, 438, width, height),
            2,
            palette_pixel(
                palette,
                if position == selected {
                    PaletteRole::Footer
                } else {
                    PaletteRole::ControlBorder
                },
            ),
        );
        x = x.saturating_add(width.saturating_add(gap));
    }
}

pub(crate) fn draw_status(canvas: &mut Canvas<'_>, status: Status, palette: &Palette) {
    let mut text = TextBuffer::<96>::new();
    match status {
        Status::Clear => return,
        Status::VolumeMuted => text.push_bytes(b"GAME VOLUME MUTED"),
        Status::Volume(percent) => {
            let _ = write!(text, "GAME VOLUME {percent}%");
        }
        Status::Brightness(percent) => {
            let _ = write!(text, "BRIGHTNESS {percent}%");
        }
        Status::Keymap(Keymap::Czech) => text.push_bytes(b"TERMINAL KEYS: CZECH"),
        Status::Keymap(Keymap::Us) => text.push_bytes(b"TERMINAL KEYS: US ANSI"),
        Status::RebootConfirmation => text.push_bytes(b"PRESS A OR TAP AGAIN TO REBOOT"),
    }
    let scale = fit_text_scale(text.as_str(), CANVAS_WIDTH.saturating_sub(220), 2, 1);
    canvas.draw_centered_text(
        Rect::new(100, 452, CANVAS_WIDTH.saturating_sub(200), 24),
        text.as_str(),
        scale,
        palette_pixel(palette, PaletteRole::Footer),
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArrowDirection {
    Left,
    Right,
}

fn draw_outline_arrow(
    canvas: &mut Canvas<'_>,
    bounds: Rect,
    direction: ArrowDirection,
    color: u16,
) {
    let Ok(center_x) = isize::try_from(bounds.x.saturating_add(bounds.width / 2)) else {
        return;
    };
    let Ok(center_y) = isize::try_from(bounds.y.saturating_add(bounds.height / 2)) else {
        return;
    };
    let mirror = if direction == ArrowDirection::Left {
        -1
    } else {
        1
    };
    let blocks = [
        (28, -2, 4, 4),
        (24, -6, 4, 4),
        (20, -10, 4, 4),
        (16, -14, 4, 4),
        (12, -18, 4, 4),
        (8, -22, 4, 10),
        (-28, -12, 36, 4),
        (-28, -8, 4, 16),
        (-28, 8, 36, 4),
        (8, 12, 4, 10),
        (12, 14, 4, 4),
        (16, 10, 4, 4),
        (20, 6, 4, 4),
        (24, 2, 4, 4),
    ];
    for (x, y, width, height) in blocks {
        let left = if mirror < 0 {
            center_x - x - width
        } else {
            center_x + x
        };
        fill_signed_rect(canvas, left, center_y + y, width, height, color);
    }
}

fn fill_signed_rect(
    canvas: &mut Canvas<'_>,
    x: isize,
    y: isize,
    width: isize,
    height: isize,
    color: u16,
) {
    let (Ok(x), Ok(y), Ok(width), Ok(height)) = (
        usize::try_from(x),
        usize::try_from(y),
        usize::try_from(width),
        usize::try_from(height),
    ) else {
        return;
    };
    canvas.fill_rect(Rect::new(x, y, width, height), color);
}

fn draw_settings_gear(canvas: &mut Canvas<'_>, bounds: Rect) {
    let target_size = 50_usize.min(bounds.width).min(bounds.height).max(1);
    let left = bounds
        .x
        .saturating_add(bounds.width.saturating_sub(target_size) / 2);
    let top = bounds
        .y
        .saturating_add(bounds.height.saturating_sub(target_size) / 2);
    let colors = [
        rgb888_to_rgb565(0x00_00_00),
        rgb888_to_rgb565(0x2e_2e_2e),
        rgb888_to_rgb565(0x72_72_72),
        rgb888_to_rgb565(0xa0_a0_a0),
    ];
    for y in 0..target_size {
        let source_y = y.saturating_mul(SETTINGS_GEAR_SIZE) / target_size;
        let row = SETTINGS_GEAR.get(source_y).copied().unwrap_or_default();
        for x in 0..target_size {
            let source_x = x.saturating_mul(SETTINGS_GEAR_SIZE) / target_size;
            let Some(shade) = row.as_bytes().get(source_x).copied() else {
                continue;
            };
            let Some(index) = shade.checked_sub(b'0').map(usize::from) else {
                continue;
            };
            if let Some(color) = colors.get(index).copied() {
                canvas.set_pixel(left.saturating_add(x), top.saturating_add(y), color);
            }
        }
    }
}

pub(crate) fn palette_pixel(palette: &Palette, role: PaletteRole) -> u16 {
    rgb_pixel(palette.color(role))
}

fn rgb_pixel(color: Rgb) -> u16 {
    components_pixel(color.components())
}

fn game_pixel(entry: &CatalogEntry) -> u16 {
    components_pixel(entry.color().components())
}

fn components_pixel(components: [u8; 3]) -> u16 {
    let [red, green, blue] = components;
    rgb888_to_rgb565((u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue))
}

#[cfg(test)]
mod tests {
    use super::{
        ArtworkProvider, CANVAS_HEIGHT, CANVAS_WIDTH, Cover, CoverError, DashboardFrame,
        EntryButton, MenuLayout, PIXELS, draw_cover_square, palette_pixel,
    };
    use crate::{
        Action, Brightness, CreditsCrawl, DashboardCatalog, DashboardModel, Keymap, VolumeState,
    };
    use retro_deck_config::{Catalog, Credits, Palette, PaletteRole};
    use retro_deck_ui::{Canvas, Rect};

    const DEPLOYED_CATALOG: &[u8] = include_bytes!("../../../deploy/menu/games.tsv");
    const DEPLOYED_CREDITS: &[u8] = include_bytes!("../../../deploy/menu/credits.tsv");

    fn model() -> Option<DashboardModel> {
        let catalog = Catalog::parse(DEPLOYED_CATALOG).ok()?;
        let catalog = DashboardCatalog::from_catalog(&catalog).ok()?;
        Some(DashboardModel::new(
            catalog,
            VolumeState::new(42, 42).ok()?,
            Brightness::new(60).ok()?,
            Keymap::Us,
        ))
    }

    fn hash(frame: &DashboardFrame) -> u64 {
        frame
            .pixels()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325, |hash, pixel| {
                (hash ^ u64::from(*pixel)).wrapping_mul(0x0000_0100_0000_01b3)
            })
    }

    #[test]
    fn renders_fixed_complete_catalog_and_hit_targets() {
        let Some(model) = model() else {
            return;
        };
        let palette = Palette::default();
        let Some(frame) = DashboardFrame::render_menu(&model, &palette).ok() else {
            return;
        };
        let Some(layout) = frame.menu_layout() else {
            return;
        };
        assert_eq!(frame.pixels().len(), PIXELS);
        assert_eq!(DashboardFrame::stride_bytes(), CANVAS_WIDTH * 2);
        assert_eq!(frame.pixel(CANVAS_WIDTH, 0), None);
        assert_eq!(frame.pixel(0, CANVAS_HEIGHT), None);
        assert_eq!(layout.action_at(20, 420), Some(Action::ShowCredits));
        assert_eq!(frame.action_at(20, 420), Some(Action::ShowCredits));
        assert_eq!(layout.action_at(1_220, 420), Some(Action::ToggleSettings));
        assert_eq!(layout.action_at(60, 80), Some(Action::SelectCategory(0)));
        assert_eq!(layout.selected_entry_index(), Some(0));
        assert_eq!(
            layout.entry_buttons()[0].map(EntryButton::entry_index),
            Some(0)
        );
        assert_eq!(
            frame.pixel(344, 410),
            Some(palette_pixel(&palette, PaletteRole::Active))
        );
        assert!(layout.previous_button().is_some());
        assert!(layout.next_button().is_some());
    }

    #[test]
    fn redraw_reuses_pixels_and_moves_the_selected_card() {
        let Some(mut model) = model() else {
            return;
        };
        let palette = Palette::default();
        let Some(mut frame) = DashboardFrame::render_menu(&model, &palette).ok() else {
            return;
        };
        let allocation = frame.pixels.as_ptr();
        let capacity = frame.pixels.capacity();
        let before = hash(&frame);

        let transition = model.apply(Action::Next);
        assert!(transition.redraw);
        frame.redraw_menu(&model, &palette);

        assert_eq!(frame.pixels.as_ptr(), allocation);
        assert_eq!(frame.pixels.capacity(), capacity);
        assert_ne!(hash(&frame), before);
        assert_eq!(
            frame
                .menu_layout()
                .and_then(MenuLayout::selected_entry_index),
            Some(1)
        );
    }

    #[test]
    fn switching_screens_reuses_one_complete_frame_allocation() {
        use crate::{NetworkView, RenderedScreen, SettingsView};

        let Some(mut model) = model() else {
            return;
        };
        let palette = Palette::default();
        let Some(mut frame) = DashboardFrame::render_menu(&model, &palette).ok() else {
            return;
        };
        let allocation = frame.pixels.as_ptr();
        let capacity = frame.pixels.capacity();
        let _ = model.apply(Action::ToggleSettings);
        frame.redraw_settings(
            &model,
            &palette,
            SettingsView::new(NetworkView::unavailable(), "/bin/ash"),
        );
        assert_eq!(frame.rendered_screen(), RenderedScreen::Settings);
        assert_eq!(frame.pixels.as_ptr(), allocation);
        assert_eq!(frame.pixels.capacity(), capacity);

        let _ = model.apply(Action::Back);
        let Some(credits) = Credits::parse(DEPLOYED_CREDITS).ok() else {
            return;
        };
        let crawl = CreditsCrawl::from_credits(&credits);
        let _ = model.apply(Action::ShowCredits);
        frame.redraw_credits(&crawl, &palette, false, 2_000);
        assert_eq!(frame.rendered_screen(), RenderedScreen::Credits);
        assert_eq!(frame.pixels.as_ptr(), allocation);
        assert_eq!(frame.pixels.capacity(), capacity);
        assert_eq!(
            frame
                .credits_layout()
                .and_then(|layout| layout.action_at(1_240, 40)),
            Some(Action::Back)
        );

        let _ = model.apply(Action::Back);
        frame.redraw_menu(&model, &palette);
        assert_eq!(frame.rendered_screen(), RenderedScreen::Menu);
        assert_eq!(frame.pixels.as_ptr(), allocation);
        assert_eq!(frame.pixels.capacity(), capacity);
        assert_eq!(hash(&frame), 9_026_554_421_741_662_983);
    }

    #[test]
    fn cover_validation_and_square_crop_are_exact() {
        assert_eq!(Cover::new(0, 1, &[]), Err(CoverError::Dimensions));
        assert_eq!(Cover::new(2, 2, &[1, 2, 3]), Err(CoverError::Storage));
        let pixels = [1, 2, 3, 4, 5, 6, 7, 8];
        let Some(cover) = Cover::new(4, 2, &pixels).ok() else {
            return;
        };
        let mut output = [0_u16; 16];
        let Some(mut canvas) = Canvas::new(&mut output, 4, 4) else {
            return;
        };
        draw_cover_square(&mut canvas, Rect::new(0, 0, 4, 4), cover);
        assert_eq!(output, [2, 2, 3, 3, 2, 2, 3, 3, 6, 6, 7, 7, 6, 6, 7, 7]);
    }

    #[test]
    fn optional_cover_provider_supplies_the_selected_art_square() {
        #[derive(Debug)]
        struct MarioCover([u16; 1]);

        impl ArtworkProvider for MarioCover {
            fn cover(&self, identifier: &str) -> Option<Cover<'_>> {
                (identifier == "mario")
                    .then(|| Cover::new(1, 1, &self.0).ok())
                    .flatten()
            }
        }

        let Some(model) = model() else {
            return;
        };
        let provider = MarioCover([0x1234]);
        let Some(frame) =
            DashboardFrame::render_menu_with_artwork(&model, &Palette::default(), &provider).ok()
        else {
            return;
        };
        assert_eq!(frame.pixel(288, 162), Some(0x1234));
        assert_eq!(frame.pixel(487, 361), Some(0x1234));
    }

    #[test]
    fn canonical_catalog_frame_has_a_stable_snapshot() {
        let Some(model) = model() else {
            return;
        };
        let Some(frame) = DashboardFrame::render_menu(&model, &Palette::default()).ok() else {
            return;
        };
        // Captured from the authoritative C++ menu renderer. The complete
        // 1280x480 frame also compares pixel-for-pixel through the PPM tool.
        assert_eq!(hash(&frame), 9_026_554_421_741_662_983);
    }
}
