//! Deterministic Wi-Fi editor rendering and hit geometry.

use std::fmt::Write as _;

use retro_deck_config::{Palette, PaletteRole};
use retro_deck_ui::{Canvas, Rect, TextBuffer, fit_text_scale};

use crate::render::{CANVAS_WIDTH, palette_pixel};
use crate::{NetworkView, WifiAction, WifiEditor, WifiField, WifiStatus};

const MAXIMUM_WIFI_KEYS: usize = 42;
const KEY_GAP: usize = 6;
const KEY_MARGIN: usize = 16;
const KEY_HEIGHT: usize = 62;
const LETTER_ROWS: [(&[u8], usize); 4] = [
    (b"qwertyuiop", 86),
    (b"asdfghjkl", 154),
    (b"zxcvbnm", 222),
    (b"@._-", 290),
];
const SYMBOL_ROWS: [(&[u8], usize); 4] = [
    (b"1234567890", 86),
    (b"!@#$%^&*()", 154),
    (b"-_=+[]{}\\|", 222),
    (b"`~;:'\",./?<>", 290),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WifiKey {
    bounds: Rect,
    value: u8,
}

/// Complete fixed-capacity touch geometry for the Wi-Fi editor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiLayout {
    back_button: Rect,
    ssid_field: Rect,
    passphrase_field: Rect,
    save_button: Rect,
    mode_button: Rect,
    shift_button: Rect,
    space_button: Rect,
    delete_button: Rect,
    keys: [Option<WifiKey>; MAXIMUM_WIFI_KEYS],
    key_count: usize,
}

impl WifiLayout {
    pub(crate) const fn empty() -> Self {
        Self {
            back_button: Rect::new(16, 10, 120, 62),
            ssid_field: Rect::new(330, 10, 310, 62),
            passphrase_field: Rect::new(650, 10, 330, 62),
            save_button: Rect::new(990, 10, 274, 62),
            mode_button: Rect::new(16, 364, 152, 66),
            shift_button: Rect::new(176, 364, 168, 66),
            space_button: Rect::new(352, 364, 700, 66),
            delete_button: Rect::new(1_060, 364, 204, 66),
            keys: [None; MAXIMUM_WIFI_KEYS],
            key_count: 0,
        }
    }

    /// Back control.
    #[must_use]
    pub const fn back_button(self) -> Rect {
        self.back_button
    }

    /// Save-profile control.
    #[must_use]
    pub const fn save_button(self) -> Rect {
        self.save_button
    }

    /// Number of printable keys on the current keyboard page.
    #[must_use]
    pub const fn key_count(self) -> usize {
        self.key_count
    }

    /// Map one committed touch coordinate to a semantic editor action.
    #[must_use]
    pub fn action_at(&self, x: usize, y: usize) -> Option<WifiAction> {
        if self.back_button.contains(x, y) {
            return Some(WifiAction::Close);
        }
        if self.ssid_field.contains(x, y) {
            return Some(WifiAction::SelectField(WifiField::Ssid));
        }
        if self.passphrase_field.contains(x, y) {
            return Some(WifiAction::SelectField(WifiField::Passphrase));
        }
        if self.save_button.contains(x, y) {
            return Some(WifiAction::Save);
        }
        if self.mode_button.contains(x, y) {
            return Some(WifiAction::ToggleSymbols);
        }
        if self.shift_button.contains(x, y) {
            return Some(WifiAction::ToggleShift);
        }
        if self.space_button.contains(x, y) {
            return Some(WifiAction::TypeAscii(b' '));
        }
        if self.delete_button.contains(x, y) {
            return Some(WifiAction::Delete);
        }
        self.keys.iter().flatten().find_map(|key| {
            key.bounds
                .contains(x, y)
                .then_some(WifiAction::TypeAscii(key.value))
        })
    }
}

pub(crate) fn draw_wifi(
    canvas: &mut Canvas<'_>,
    editor: &WifiEditor,
    network: NetworkView<'_>,
    palette: &Palette,
) -> WifiLayout {
    canvas.clear(palette_pixel(palette, PaletteRole::Background));
    let mut layout = WifiLayout::empty();
    draw_button(canvas, layout.back_button, "BACK", false, palette);
    canvas.draw_text(
        158,
        25,
        "ADD WIFI",
        3,
        palette_pixel(palette, PaletteRole::Title),
    );
    draw_fields(canvas, editor, &layout, palette);
    draw_button(canvas, layout.save_button, "SAVE NETWORK", false, palette);

    let rows = if editor.symbols() {
        &SYMBOL_ROWS
    } else {
        &LETTER_ROWS
    };
    for (values, y) in rows {
        add_key_row(
            canvas,
            values,
            *y,
            editor.uppercase() && !editor.symbols(),
            &mut layout,
            palette,
        );
    }

    draw_button(
        canvas,
        layout.mode_button,
        if editor.symbols() { "ABC" } else { "123" },
        editor.symbols(),
        palette,
    );
    draw_button(
        canvas,
        layout.shift_button,
        if editor.uppercase() { "ABC" } else { "abc" },
        !editor.symbols() && editor.uppercase(),
        palette,
    );
    draw_button(canvas, layout.space_button, "SPACE", false, palette);
    draw_button(canvas, layout.delete_button, "DELETE", false, palette);
    draw_footer(canvas, editor.status(), network, palette);
    layout
}

fn draw_fields(
    canvas: &mut Canvas<'_>,
    editor: &WifiEditor,
    layout: &WifiLayout,
    palette: &Palette,
) {
    let field = palette_pixel(palette, PaletteRole::Field);
    let field_label = palette_pixel(palette, PaletteRole::FieldLabel);
    let text = palette_pixel(palette, PaletteRole::White);

    canvas.fill_rect(layout.ssid_field, field);
    canvas.stroke_rect(
        layout.ssid_field,
        3,
        palette_pixel(
            palette,
            if editor.field() == WifiField::Ssid {
                PaletteRole::WifiFocus
            } else {
                PaletteRole::InactiveBorder
            },
        ),
    );
    canvas.draw_text(
        layout.ssid_field.x.saturating_add(10),
        layout.ssid_field.y.saturating_add(7),
        "SSID",
        1,
        field_label,
    );
    let ssid = tail_text(editor.ssid().as_bytes(), 19);
    canvas.draw_text(
        layout.ssid_field.x.saturating_add(10),
        layout.ssid_field.y.saturating_add(28),
        ssid.as_str(),
        2,
        text,
    );

    canvas.fill_rect(layout.passphrase_field, field);
    canvas.stroke_rect(
        layout.passphrase_field,
        3,
        palette_pixel(
            palette,
            if editor.field() == WifiField::Passphrase {
                PaletteRole::WifiFocus
            } else {
                PaletteRole::InactiveBorder
            },
        ),
    );
    canvas.draw_text(
        layout.passphrase_field.x.saturating_add(10),
        layout.passphrase_field.y.saturating_add(7),
        "PASSWORD",
        1,
        field_label,
    );
    let password = masked_password(editor.passphrase_len(), 20);
    canvas.draw_text(
        layout.passphrase_field.x.saturating_add(10),
        layout.passphrase_field.y.saturating_add(28),
        password.as_str(),
        2,
        text,
    );
}

fn add_key_row(
    canvas: &mut Canvas<'_>,
    values: &[u8],
    y: usize,
    uppercase: bool,
    layout: &mut WifiLayout,
    palette: &Palette,
) {
    if values.is_empty() {
        return;
    }
    let count = values.len();
    let gaps = KEY_GAP.saturating_mul(count.saturating_sub(1));
    let width = CANVAS_WIDTH
        .saturating_sub(KEY_MARGIN.saturating_mul(2))
        .saturating_sub(gaps)
        / count;
    let used = count
        .saturating_mul(width)
        .saturating_add(KEY_GAP.saturating_mul(count.saturating_sub(1)));
    let left = CANVAS_WIDTH.saturating_sub(used) / 2;
    for (index, value) in values.iter().copied().enumerate() {
        let value = if uppercase && value.is_ascii_lowercase() {
            value.to_ascii_uppercase()
        } else {
            value
        };
        let bounds = Rect::new(
            left.saturating_add(index.saturating_mul(width.saturating_add(KEY_GAP))),
            y,
            width,
            KEY_HEIGHT,
        );
        let Some(slot) = layout.keys.get_mut(layout.key_count) else {
            return;
        };
        *slot = Some(WifiKey { bounds, value });
        layout.key_count = layout.key_count.saturating_add(1);
        let label = [value];
        let label = std::str::from_utf8(&label).unwrap_or("?");
        draw_button(canvas, bounds, label, false, palette);
    }
}

fn draw_button(
    canvas: &mut Canvas<'_>,
    bounds: Rect,
    label: &str,
    active: bool,
    palette: &Palette,
) {
    canvas.fill_rect(
        bounds,
        palette_pixel(
            palette,
            if active {
                PaletteRole::WifiActive
            } else {
                PaletteRole::Surface
            },
        ),
    );
    canvas.stroke_rect(
        bounds,
        3,
        palette_pixel(
            palette,
            if active {
                PaletteRole::WifiActiveBorder
            } else {
                PaletteRole::ControlBorder
            },
        ),
    );
    canvas.draw_centered_text(
        bounds,
        label,
        fit_text_scale(label, bounds.width.saturating_sub(12), 3, 1),
        palette_pixel(palette, PaletteRole::White),
    );
}

fn draw_footer(
    canvas: &mut Canvas<'_>,
    status: WifiStatus,
    network: NetworkView<'_>,
    palette: &Palette,
) {
    canvas.draw_centered_text(
        Rect::new(12, 436, CANVAS_WIDTH.saturating_sub(24), 10),
        status_text(status),
        1,
        palette_pixel(palette, PaletteRole::Footer),
    );

    let mut addresses = TextBuffer::<256>::new();
    let _ = write!(
        addresses,
        "WIFI {}  WLAN0 {}  WG0 {}",
        shown(network.ssid(), "NOT CONNECTED"),
        shown(network.wlan_ipv4(), "NO ADDRESS"),
        shown(network.wireguard_ipv4(), "NO ADDRESS")
    );
    addresses.fit_width(CANVAS_WIDTH.saturating_sub(32), 1);
    canvas.draw_centered_text(
        Rect::new(12, 450, CANVAS_WIDTH.saturating_sub(24), 10),
        addresses.as_str(),
        1,
        palette_pixel(palette, PaletteRole::Text),
    );

    let mut selector = TextBuffer::<128>::from_bytes(b"AUTO WIFI: ");
    selector.push_display(network.selector());
    selector.fit_width(CANVAS_WIDTH.saturating_sub(32), 1);
    canvas.draw_centered_text(
        Rect::new(12, 464, CANVAS_WIDTH.saturating_sub(24), 10),
        selector.as_str(),
        1,
        palette_pixel(palette, PaletteRole::Muted),
    );
}

const fn status_text(status: WifiStatus) -> &'static str {
    match status {
        WifiStatus::Clear => "SAVING DOES NOT INTERRUPT CURRENT WIFI",
        WifiStatus::InvalidSsid => "SSID MUST BE 1 TO 32 CHARACTERS",
        WifiStatus::InvalidPassphrase => "PASSWORD MUST BE 8 TO 63 CHARACTERS",
        WifiStatus::Saving => "SAVING WIFI PROFILE",
        WifiStatus::Saved => "WIFI SAVED - USED AFTER CURRENT WIFI DISCONNECTS",
        WifiStatus::SaveFailed => "WIFI PROFILE WAS NOT SAVED",
    }
}

const fn shown<'text>(value: &'text str, fallback: &'text str) -> &'text str {
    if value.is_empty() { fallback } else { value }
}

fn tail_text(bytes: &[u8], maximum: usize) -> TextBuffer<32> {
    if bytes.len() <= maximum {
        return TextBuffer::from_bytes(bytes);
    }
    if maximum <= 3 {
        let start = bytes.len().saturating_sub(maximum);
        return TextBuffer::from_bytes(bytes.get(start..).unwrap_or_default());
    }
    let mut output = TextBuffer::from_bytes(b"...");
    let start = bytes.len().saturating_sub(maximum.saturating_sub(3));
    output.push_bytes(bytes.get(start..).unwrap_or_default());
    output
}

fn masked_password(length: usize, maximum: usize) -> TextBuffer<32> {
    let mut output = TextBuffer::new();
    if length > maximum && maximum > 3 {
        output.push_bytes(b"...");
        for _ in 0..maximum.saturating_sub(3) {
            output.push_bytes(b"*");
        }
    } else {
        for _ in 0..length.min(maximum) {
            output.push_bytes(b"*");
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use retro_deck_ui::{Canvas, Rect};

    use super::{CANVAS_WIDTH, WifiAction, WifiEditor, WifiLayout, draw_wifi};
    use crate::NetworkView;
    use retro_deck_config::Palette;

    const HEIGHT: usize = 480;

    #[test]
    fn keyboard_pages_have_closed_exact_hit_targets() {
        let mut pixels = vec![0_u16; CANVAS_WIDTH * HEIGHT];
        let Some(mut canvas) = Canvas::new(&mut pixels, CANVAS_WIDTH, HEIGHT) else {
            return;
        };
        let mut editor = WifiEditor::new();
        let palette = Palette::default();
        let network = NetworkView::unavailable();
        let lowercase = draw_wifi(&mut canvas, &editor, network, &palette);
        assert_eq!(lowercase.key_count(), 30);
        assert_eq!(lowercase.action_at(20, 20), Some(WifiAction::Close));
        assert_eq!(lowercase.action_at(1_000, 20), Some(WifiAction::Save));
        assert_eq!(
            first_key_action(&lowercase),
            Some(WifiAction::TypeAscii(b'q'))
        );

        let _ = editor.apply(WifiAction::ToggleShift);
        let uppercase = draw_wifi(&mut canvas, &editor, network, &palette);
        assert_eq!(
            first_key_action(&uppercase),
            Some(WifiAction::TypeAscii(b'Q'))
        );
        let _ = editor.apply(WifiAction::ToggleSymbols);
        let symbols = draw_wifi(&mut canvas, &editor, network, &palette);
        assert_eq!(symbols.key_count(), 42);
        assert_eq!(
            first_key_action(&symbols),
            Some(WifiAction::TypeAscii(b'1'))
        );
    }

    fn first_key_action(layout: &WifiLayout) -> Option<WifiAction> {
        for y in 86..148 {
            for x in 0..CANVAS_WIDTH {
                if let Some(action @ WifiAction::TypeAscii(_)) = layout.action_at(x, y) {
                    return Some(action);
                }
            }
        }
        None
    }

    #[test]
    fn fixed_controls_do_not_overlap() {
        let layout = WifiLayout::empty();
        assert!(!overlaps(layout.back_button(), layout.save_button()));
    }

    #[test]
    fn canonical_editor_pages_have_stable_pixels() {
        let palette = Palette::default();
        let network = NetworkView::new("STUDIO", "192.0.2.20", "198.51.100.10", "CONNECTED");
        let mut editor = WifiEditor::new();
        let lowercase = render_hash(&editor, network, &palette);
        let _ = editor.apply(WifiAction::ToggleShift);
        let uppercase = render_hash(&editor, network, &palette);
        for byte in b"NETWORK" {
            let _ = editor.apply(WifiAction::TypeAscii(*byte));
        }
        let _ = editor.apply(WifiAction::SelectField(crate::WifiField::Passphrase));
        for byte in b"password" {
            let _ = editor.apply(WifiAction::TypeAscii(*byte));
        }
        let password = render_hash(&editor, network, &palette);
        let _ = editor.apply(WifiAction::ToggleShift);
        let _ = editor.apply(WifiAction::ToggleSymbols);
        let symbols = render_hash(&editor, network, &palette);
        assert_eq!(
            [lowercase, uppercase, password, symbols],
            [
                11_395_240_426_197_528_069,
                9_675_268_873_608_716_987,
                13_668_664_515_746_998_199,
                7_848_944_006_897_503_067,
            ]
        );
    }

    fn render_hash(editor: &WifiEditor, network: NetworkView<'_>, palette: &Palette) -> u64 {
        let mut pixels = vec![0_u16; CANVAS_WIDTH * HEIGHT];
        let Some(mut canvas) = Canvas::new(&mut pixels, CANVAS_WIDTH, HEIGHT) else {
            return 0;
        };
        let _layout = draw_wifi(&mut canvas, editor, network, palette);
        pixels.iter().fold(0xcbf2_9ce4_8422_2325, |hash, pixel| {
            (hash ^ u64::from(*pixel)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    const fn overlaps(left: Rect, right: Rect) -> bool {
        left.x < right.x.saturating_add(right.width)
            && right.x < left.x.saturating_add(left.width)
            && left.y < right.y.saturating_add(right.height)
            && right.y < left.y.saturating_add(left.height)
    }
}
