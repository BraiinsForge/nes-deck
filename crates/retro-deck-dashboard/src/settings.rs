//! Settings-screen rendering over read-only device status.

use std::fmt::Write as _;

use retro_deck_config::{Palette, PaletteRole};
use retro_deck_ui::{Canvas, Rect, TextBuffer};

use crate::{Action, DashboardModel, Keymap, SettingsTarget};

use crate::render::{draw_status, palette_pixel};

/// Read-only network values collected outside the dashboard model and renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkView<'text> {
    ssid: &'text str,
    wlan_ipv4: &'text str,
    wireguard_ipv4: &'text str,
    selector: &'text str,
}

impl<'text> NetworkView<'text> {
    /// Borrow one immutable network snapshot.
    #[must_use]
    pub const fn new(
        ssid: &'text str,
        wlan_ipv4: &'text str,
        wireguard_ipv4: &'text str,
        selector: &'text str,
    ) -> Self {
        Self {
            ssid,
            wlan_ipv4,
            wireguard_ipv4,
            selector,
        }
    }

    /// A safe empty snapshot when status collection is unavailable.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self::new("", "", "", "STATUS UNAVAILABLE")
    }
}

/// Device text needed only while drawing settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsView<'text> {
    network: NetworkView<'text>,
    login_shell: &'text str,
}

impl<'text> SettingsView<'text> {
    /// Construct a read-only settings view.
    #[must_use]
    pub const fn new(network: NetworkView<'text>, login_shell: &'text str) -> Self {
        Self {
            network,
            login_shell,
        }
    }
}

/// Exact settings hit targets derived from the rendered controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsLayout;

impl SettingsLayout {
    pub(crate) const fn fixed() -> Self {
        Self
    }

    /// Close control in the top-right corner.
    #[must_use]
    pub const fn close_button(self) -> Rect {
        Rect::new(1_212, 12, 56, 56)
    }

    /// Wi-Fi editor control and status summary.
    #[must_use]
    pub const fn wifi_button(self) -> Rect {
        Rect::new(926, 20, 262, 108)
    }

    const fn volume_down_button() -> Rect {
        Rect::new(108, 208, 104, 104)
    }

    const fn volume_up_button() -> Rect {
        Rect::new(228, 208, 104, 104)
    }

    const fn volume_label() -> Rect {
        Rect::new(82, 328, 276, 66)
    }

    const fn brightness_down_button() -> Rect {
        Rect::new(438, 208, 104, 104)
    }

    const fn brightness_up_button() -> Rect {
        Rect::new(558, 208, 104, 104)
    }

    const fn terminal_button() -> Rect {
        Rect::new(792, 208, 112, 104)
    }

    const fn keymap_button() -> Rect {
        Rect::new(1_036, 208, 112, 104)
    }

    /// Map one committed touch coordinate to a semantic model action.
    #[must_use]
    pub const fn action_at(self, x: usize, y: usize) -> Option<Action> {
        if self.close_button().contains(x, y) {
            Some(Action::Back)
        } else if Self::volume_down_button().contains(x, y) {
            Some(Action::ActivateSettings(SettingsTarget::VolumeDown))
        } else if Self::volume_up_button().contains(x, y) {
            Some(Action::ActivateSettings(SettingsTarget::VolumeUp))
        } else if Self::volume_label().contains(x, y) {
            Some(Action::ToggleMute)
        } else if Self::brightness_down_button().contains(x, y) {
            Some(Action::ActivateSettings(SettingsTarget::BrightnessDown))
        } else if Self::brightness_up_button().contains(x, y) {
            Some(Action::ActivateSettings(SettingsTarget::BrightnessUp))
        } else if Self::terminal_button().contains(x, y) {
            Some(Action::ActivateSettings(SettingsTarget::Terminal))
        } else if Self::keymap_button().contains(x, y) {
            Some(Action::ActivateSettings(SettingsTarget::Keymap))
        } else if self.wifi_button().contains(x, y) {
            Some(Action::ActivateSettings(SettingsTarget::Wifi))
        } else {
            None
        }
    }
}

pub(crate) fn draw_settings(
    canvas: &mut Canvas<'_>,
    model: &DashboardModel,
    palette: &Palette,
    view: SettingsView<'_>,
) -> SettingsLayout {
    canvas.clear(palette_pixel(palette, PaletteRole::Background));
    let layout = SettingsLayout::fixed();
    draw_close_icon(
        canvas,
        layout.close_button(),
        palette_pixel(palette, PaletteRole::Text),
    );
    draw_network_status(
        canvas,
        layout,
        palette,
        view.network,
        model.settings_target() == SettingsTarget::Wifi,
    );
    draw_settings_controls(canvas, model, palette);
    draw_settings_labels(canvas, layout, model, palette, view.login_shell);
    draw_status(canvas, model.status(), palette);
    layout
}

fn draw_network_status(
    canvas: &mut Canvas<'_>,
    layout: SettingsLayout,
    palette: &Palette,
    network: NetworkView<'_>,
    selected: bool,
) {
    let muted = palette_pixel(palette, PaletteRole::Muted);
    let text = palette_pixel(palette, PaletteRole::Text);
    canvas.draw_text(64, 22, "ACTIVE WIFI", 1, muted);
    let mut ssid = TextBuffer::<64>::from_display(if network.ssid.is_empty() {
        "NOT CONNECTED"
    } else {
        network.ssid
    });
    ssid.fit_width(300, 3);
    canvas.draw_text(64, 44, ssid.as_str(), 3, text);

    canvas.draw_text(392, 22, "WLAN0", 1, muted);
    draw_address(canvas, 392, network.wlan_ipv4, text);
    canvas.draw_text(620, 22, "WIREGUARD", 1, muted);
    draw_address(canvas, 620, network.wireguard_ipv4, text);

    let mut selector = TextBuffer::<96>::from_bytes(b"AUTO WIFI: ");
    selector.push_display(network.selector);
    selector.fit_width(790, 1);
    canvas.draw_text(
        64,
        88,
        selector.as_str(),
        1,
        palette_pixel(palette, PaletteRole::Footer),
    );

    draw_settings_control(canvas, layout.wifi_button(), selected, palette);
    draw_wifi_icon(
        canvas,
        Rect::new(
            layout.wifi_button().x.saturating_add(12),
            layout.wifi_button().y.saturating_add(24),
            54,
            54,
        ),
        text,
    );
    canvas.draw_text(
        layout.wifi_button().x.saturating_add(78),
        layout.wifi_button().y.saturating_add(28),
        "WIFI",
        3,
        text,
    );
    canvas.draw_text(
        layout.wifi_button().x.saturating_add(78),
        layout.wifi_button().y.saturating_add(64),
        "SETTINGS",
        2,
        muted,
    );
}

fn draw_address(canvas: &mut Canvas<'_>, x: usize, address: &str, color: u16) {
    let mut shown = TextBuffer::<48>::from_display(if address.is_empty() {
        "NO ADDRESS"
    } else {
        address
    });
    shown.fit_width(200, 2);
    canvas.draw_text(x, 44, shown.as_str(), 2, color);
}

fn draw_settings_controls(canvas: &mut Canvas<'_>, model: &DashboardModel, palette: &Palette) {
    let selected = model.settings_target();
    draw_settings_control(
        canvas,
        SettingsLayout::volume_down_button(),
        selected == SettingsTarget::VolumeDown,
        palette,
    );
    draw_settings_control(
        canvas,
        SettingsLayout::volume_up_button(),
        selected == SettingsTarget::VolumeUp,
        palette,
    );
    let text = palette_pixel(palette, PaletteRole::Text);
    draw_speaker_icon(canvas, SettingsLayout::volume_down_button(), false, text);
    draw_speaker_icon(canvas, SettingsLayout::volume_up_button(), true, text);

    draw_settings_control(
        canvas,
        SettingsLayout::brightness_down_button(),
        selected == SettingsTarget::BrightnessDown,
        palette,
    );
    draw_settings_control(
        canvas,
        SettingsLayout::brightness_up_button(),
        selected == SettingsTarget::BrightnessUp,
        palette,
    );
    draw_sun_icon(
        canvas,
        SettingsLayout::brightness_down_button(),
        false,
        text,
    );
    draw_sun_icon(canvas, SettingsLayout::brightness_up_button(), true, text);

    draw_settings_control(
        canvas,
        SettingsLayout::terminal_button(),
        selected == SettingsTarget::Terminal,
        palette,
    );
    draw_terminal_icon(canvas, SettingsLayout::terminal_button(), text);
    draw_settings_control(
        canvas,
        SettingsLayout::keymap_button(),
        selected == SettingsTarget::Keymap,
        palette,
    );
    canvas.draw_centered_text(
        SettingsLayout::keymap_button(),
        if model.keymap() == Keymap::Czech {
            "CZ"
        } else {
            "EN"
        },
        4,
        text,
    );
}

fn draw_settings_control(canvas: &mut Canvas<'_>, bounds: Rect, selected: bool, palette: &Palette) {
    canvas.draw_panel(
        bounds,
        palette_pixel(
            palette,
            if selected {
                PaletteRole::Active
            } else {
                PaletteRole::ControlSurface
            },
        ),
        palette_pixel(
            palette,
            if selected {
                PaletteRole::Accent
            } else {
                PaletteRole::ControlBorder
            },
        ),
        4,
    );
}

fn draw_settings_labels(
    canvas: &mut Canvas<'_>,
    _layout: SettingsLayout,
    model: &DashboardModel,
    palette: &Palette,
    login_shell: &str,
) {
    let text = palette_pixel(palette, PaletteRole::Text);
    let muted = palette_pixel(palette, PaletteRole::Muted);
    let mut volume = TextBuffer::<4>::new();
    if model.volume().is_muted() {
        volume.push_bytes(b"OFF");
    } else {
        let _ = write!(volume, "{}", model.volume().percent());
    }
    canvas.draw_centered_text(Rect::new(82, 328, 276, 34), volume.as_str(), 3, text);
    canvas.draw_centered_text(Rect::new(82, 366, 276, 28), "VOLUME", 2, muted);

    let mut brightness = TextBuffer::<4>::new();
    let _ = write!(brightness, "{}", model.brightness().percent());
    canvas.draw_centered_text(Rect::new(412, 328, 276, 34), brightness.as_str(), 3, text);
    canvas.draw_centered_text(Rect::new(412, 366, 276, 28), "BRIGHTNESS", 2, muted);

    canvas.draw_centered_text(Rect::new(750, 328, 196, 34), "TERMINAL", 3, text);
    let mut shell = TextBuffer::<32>::from_display(login_shell);
    shell.fit_width(196, 2);
    canvas.draw_centered_text(Rect::new(750, 366, 196, 28), shell.as_str(), 2, muted);

    canvas.draw_centered_text(Rect::new(994, 328, 196, 34), "KEYS", 3, text);
    canvas.draw_centered_text(
        Rect::new(994, 366, 196, 28),
        if model.keymap() == Keymap::Czech {
            "CZECH"
        } else {
            "US ANSI"
        },
        2,
        muted,
    );
}

fn draw_close_icon(canvas: &mut Canvas<'_>, bounds: Rect, color: u16) {
    let center_x = bounds.x.saturating_add(bounds.width / 2);
    let center_y = bounds.y.saturating_add(bounds.height / 2);
    let offsets = [0, 4, 8, 12, 16];
    for offset in offsets {
        for (x, y) in [
            (
                center_x.saturating_add(offset),
                center_y.saturating_add(offset),
            ),
            (
                center_x.saturating_sub(offset),
                center_y.saturating_sub(offset),
            ),
            (
                center_x.saturating_add(offset),
                center_y.saturating_sub(offset),
            ),
            (
                center_x.saturating_sub(offset),
                center_y.saturating_add(offset),
            ),
        ] {
            canvas.fill_rect(Rect::new(x, y, 4, 4), color);
        }
    }
}

fn draw_wifi_icon(canvas: &mut Canvas<'_>, button: Rect, color: u16) {
    let center_x = button.x.saturating_add(button.width / 2);
    let top = button.y.saturating_add(5);
    canvas.fill_rect(Rect::new(center_x.saturating_sub(6), top, 12, 5), color);
    canvas.fill_rect(
        Rect::new(center_x.saturating_sub(12), top.saturating_add(5), 6, 5),
        color,
    );
    canvas.fill_rect(
        Rect::new(center_x.saturating_add(6), top.saturating_add(5), 6, 5),
        color,
    );
    canvas.fill_rect(
        Rect::new(center_x.saturating_sub(18), top.saturating_add(10), 6, 5),
        color,
    );
    canvas.fill_rect(
        Rect::new(center_x.saturating_add(12), top.saturating_add(10), 6, 5),
        color,
    );
    canvas.fill_rect(
        Rect::new(center_x.saturating_sub(6), top.saturating_add(22), 12, 5),
        color,
    );
    canvas.fill_rect(
        Rect::new(center_x.saturating_sub(12), top.saturating_add(27), 6, 5),
        color,
    );
    canvas.fill_rect(
        Rect::new(center_x.saturating_add(6), top.saturating_add(27), 6, 5),
        color,
    );
    canvas.fill_rect(
        Rect::new(center_x.saturating_sub(3), top.saturating_add(36), 6, 6),
        color,
    );
}

fn draw_speaker_icon(canvas: &mut Canvas<'_>, bounds: Rect, loud: bool, color: u16) {
    let x = bounds.x.saturating_add(24);
    let y = bounds.y.saturating_add(bounds.height / 2);
    canvas.fill_rect(Rect::new(x, y.saturating_sub(12), 12, 24), color);
    canvas.fill_rect(
        Rect::new(x.saturating_add(12), y.saturating_sub(20), 12, 40),
        color,
    );
    canvas.fill_rect(
        Rect::new(x.saturating_add(24), y.saturating_sub(28), 8, 56),
        color,
    );
    canvas.fill_rect(
        Rect::new(x.saturating_add(40), y.saturating_sub(16), 4, 32),
        color,
    );
    canvas.fill_rect(
        Rect::new(x.saturating_add(44), y.saturating_sub(12), 4, 24),
        color,
    );
    if loud {
        canvas.fill_rect(
            Rect::new(x.saturating_add(56), y.saturating_sub(24), 4, 48),
            color,
        );
        canvas.fill_rect(
            Rect::new(x.saturating_add(60), y.saturating_sub(16), 4, 32),
            color,
        );
    }
}

fn draw_sun_icon(canvas: &mut Canvas<'_>, bounds: Rect, bright: bool, color: u16) {
    let center_x = bounds.x.saturating_add(bounds.width / 2);
    let center_y = bounds.y.saturating_add(bounds.height / 2);
    let half = if bright { 16 } else { 12 };
    canvas.fill_pixel_cut_rect(
        Rect::new(
            center_x.saturating_sub(half),
            center_y.saturating_sub(half),
            half.saturating_mul(2),
            half.saturating_mul(2),
        ),
        4,
        color,
    );
    let reach = if bright { 34 } else { 28 };
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_sub(3),
            center_y.saturating_sub(reach),
            6,
            10,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_sub(3),
            center_y.saturating_add(reach).saturating_sub(10),
            6,
            10,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_sub(reach),
            center_y.saturating_sub(3),
            10,
            6,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            center_x.saturating_add(reach).saturating_sub(10),
            center_y.saturating_sub(3),
            10,
            6,
        ),
        color,
    );
    if bright {
        for (x, y) in [
            (center_x.saturating_sub(25), center_y.saturating_sub(25)),
            (center_x.saturating_add(18), center_y.saturating_sub(25)),
            (center_x.saturating_sub(25), center_y.saturating_add(18)),
            (center_x.saturating_add(18), center_y.saturating_add(18)),
        ] {
            canvas.fill_rect(Rect::new(x, y, 7, 7), color);
        }
    }
}

fn draw_terminal_icon(canvas: &mut Canvas<'_>, button: Rect, color: u16) {
    let icon_height = 44;
    let icon_top = button
        .y
        .saturating_add(button.height.saturating_sub(icon_height) / 2);
    let screen = Rect::new(
        button.x.saturating_add(button.width.saturating_sub(46) / 2),
        icon_top,
        46,
        34,
    );
    canvas.stroke_rect(screen, 3, color);
    canvas.fill_rect(
        Rect::new(
            button.x.saturating_add(button.width / 2).saturating_sub(3),
            icon_top.saturating_add(34),
            6,
            7,
        ),
        color,
    );
    canvas.fill_rect(
        Rect::new(
            button.x.saturating_add(24),
            icon_top.saturating_add(41),
            button.width.saturating_sub(48),
            3,
        ),
        color,
    );
    canvas.draw_text(
        screen.x.saturating_add(7),
        screen.y.saturating_add(9),
        ">_",
        2,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::{NetworkView, SettingsLayout, SettingsView, draw_settings};
    use crate::{
        Action, Brightness, CANVAS_HEIGHT, CANVAS_WIDTH, DashboardCatalog, DashboardFrame,
        DashboardModel, Intent, Keymap, RenderedScreen, SettingsTarget, VolumeState,
    };
    use retro_deck_config::{Catalog, Palette};
    use retro_deck_ui::Canvas;

    const DEPLOYED_CATALOG: &[u8] = include_bytes!("../../../deploy/menu/games.tsv");

    fn model() -> Option<DashboardModel> {
        let catalog = Catalog::parse(DEPLOYED_CATALOG).ok()?;
        let catalog = DashboardCatalog::from_catalog(&catalog).ok()?;
        let mut model = DashboardModel::new(
            catalog,
            VolumeState::new(42, 42).ok()?,
            Brightness::new(60).ok()?,
            Keymap::Us,
        );
        let transition = model.apply(Action::ToggleSettings);
        transition.redraw.then_some(model)
    }

    fn sample_view() -> SettingsView<'static> {
        SettingsView::new(
            NetworkView::new("STUDIO", "192.0.2.20", "198.51.100.10", "CONNECTED"),
            "/BIN/ASH",
        )
    }

    fn hash(pixels: &[u16]) -> u64 {
        pixels.iter().fold(0xcbf2_9ce4_8422_2325, |hash, pixel| {
            (hash ^ u64::from(*pixel)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    #[test]
    fn settings_targets_map_only_to_semantic_actions() {
        let layout = SettingsLayout::fixed();
        assert_eq!(layout.action_at(1_220, 20), Some(Action::Back));
        assert_eq!(
            layout.action_at(940, 30),
            Some(Action::ActivateSettings(SettingsTarget::Wifi))
        );
        assert_eq!(layout.action_at(100, 350), Some(Action::ToggleMute));
        assert_eq!(layout.action_at(0, 0), None);

        let Some(mut model) = model() else {
            return;
        };
        let Some(action) = layout.action_at(940, 30) else {
            return;
        };
        assert_eq!(model.apply(action).intent, Some(Intent::OpenWifi));
    }

    #[test]
    fn settings_render_is_fixed_and_case_sensitive() {
        let Some(model) = model() else {
            return;
        };
        let mut upper_pixels = vec![0_u16; CANVAS_WIDTH * CANVAS_HEIGHT];
        let Some(mut upper) = Canvas::new(&mut upper_pixels, CANVAS_WIDTH, CANVAS_HEIGHT) else {
            return;
        };
        let _ = draw_settings(&mut upper, &model, &Palette::default(), sample_view());

        let mut lower_pixels = vec![0_u16; CANVAS_WIDTH * CANVAS_HEIGHT];
        let Some(mut lower) = Canvas::new(&mut lower_pixels, CANVAS_WIDTH, CANVAS_HEIGHT) else {
            return;
        };
        let lower_view = SettingsView::new(
            NetworkView::new("Studio", "192.0.2.20", "198.51.100.10", "connected"),
            "/bin/ash",
        );
        let _ = draw_settings(&mut lower, &model, &Palette::default(), lower_view);
        assert_ne!(upper_pixels, lower_pixels);
    }

    #[test]
    fn canonical_settings_frame_has_stable_pixels_and_layout() {
        let Some(model) = model() else {
            return;
        };
        let Some(frame) =
            DashboardFrame::render_settings(&model, &Palette::default(), sample_view()).ok()
        else {
            return;
        };
        assert_eq!(frame.rendered_screen(), RenderedScreen::Settings);
        assert!(frame.menu_layout().is_none());
        assert!(frame.settings_layout().is_some());
        // Captured from the authoritative C++ settings renderer. The PPM
        // capture also compares pixel-for-pixel with its PNG output.
        assert_eq!(hash(frame.pixels()), 3_574_265_666_784_184_076);
    }
}
