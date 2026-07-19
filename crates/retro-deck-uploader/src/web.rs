//! Dependency-free rendering for the authenticated paper-style web UI.

use crate::palette::PaletteField;

/// Complete page state for either sign-in or the authenticated dashboard.
#[derive(Debug)]
pub struct Page<'a> {
    content: PageContent<'a>,
    error: Option<&'a str>,
    notice: Option<&'a str>,
}

#[derive(Debug)]
enum PageContent<'a> {
    Login,
    Dashboard {
        csrf: &'a str,
        palette: &'a [PaletteField],
    },
}

impl<'a> Page<'a> {
    /// Construct the unauthenticated sign-in page.
    #[must_use]
    pub const fn login(error: Option<&'a str>) -> Self {
        Self {
            content: PageContent::Login,
            error,
            notice: None,
        }
    }

    /// Construct the authenticated ROM and appearance dashboard.
    #[must_use]
    pub const fn dashboard(
        csrf: &'a str,
        palette: &'a [PaletteField],
        error: Option<&'a str>,
        notice: Option<&'a str>,
    ) -> Self {
        Self {
            content: PageContent::Dashboard { csrf, palette },
            error,
            notice,
        }
    }

    /// Render one complete HTML document with all dynamic text escaped.
    #[must_use]
    pub fn render(&self) -> String {
        let palette_script = matches!(
            self.content,
            PageContent::Dashboard { palette, .. } if !palette.is_empty()
        );
        let mut output = String::with_capacity(16 * 1024);
        output.push_str(
            "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <meta name=\"robots\" content=\"noindex, nofollow\">\n  <title>Retro Deck ROM uploader</title>\n  <link rel=\"stylesheet\" href=\"/assets/paper.css\">\n",
        );
        if palette_script {
            output.push_str("  <script src=\"/assets/palette.js\" defer></script>\n");
        }
        output.push_str(
            "</head>\n<body>\n  <a class=\"skip-link\" href=\"#main-content\">Skip to content</a>\n  <main id=\"main-content\">\n    <header>\n      <h1>ROM uploader</h1>\n",
        );
        match &self.content {
            PageContent::Login => self.render_login(&mut output),
            PageContent::Dashboard { csrf, palette } => {
                self.render_dashboard(&mut output, csrf, palette);
            }
        }
        output.push_str("  </main>\n</body>\n</html>\n");
        output
    }

    fn render_login(&self, output: &mut String) {
        output.push_str("    </header>\n");
        if let Some(error) = self.error {
            output
                .push_str("    <p class=\"message error\" role=\"alert\"><strong>Error:</strong> ");
            push_escaped(output, error);
            output.push_str("</p>\n");
        }
        output.push_str(
            "    <form action=\"/login\" method=\"post\">\n      <label>\n        <span>Password</span>\n        <input type=\"password\" name=\"password\" minlength=\"8\" maxlength=\"128\" autocomplete=\"current-password\" required autofocus>\n      </label>\n      <button type=\"submit\" class=\"primary-button\">Sign in</button>\n    </form>\n",
        );
    }

    fn render_dashboard(&self, output: &mut String, csrf: &str, palette: &[PaletteField]) {
        output.push_str(
            "      <form action=\"/logout\" method=\"post\">\n        <input type=\"hidden\" name=\"csrf\" value=\"",
        );
        push_escaped(output, csrf);
        output.push_str(
            "\">\n        <button type=\"submit\" class=\"quiet-button\">Sign out</button>\n      </form>\n    </header>\n",
        );
        if let Some(error) = self.error {
            output.push_str(
                "    <p class=\"message error\" role=\"alert\"><strong>Not filed:</strong> ",
            );
            push_escaped(output, error);
            output.push_str("</p>\n");
        }
        if let Some(notice) = self.notice {
            output.push_str("    <p class=\"message\" role=\"status\"><strong>Filed:</strong> ");
            push_escaped(output, notice);
            output.push_str("</p>\n");
        }
        output.push_str(
            "    <form action=\"/upload\" method=\"post\" enctype=\"multipart/form-data\">\n      <input type=\"hidden\" name=\"csrf\" value=\"",
        );
        push_escaped(output, csrf);
        output.push_str(
            "\">\n      <label>\n        <span>Console</span>\n        <select name=\"system\" required>\n          <option value=\"nes\">NES</option>\n          <option value=\"gb\">Game Boy</option>\n          <option value=\"gbc\">Game Boy Color</option>\n          <option value=\"zx\">ZX Spectrum</option>\n          <option value=\"chip8\">CHIP-8</option>\n        </select>\n      </label>\n      <label>\n        <span>Game name</span>\n        <input type=\"text\" name=\"title\" minlength=\"1\" maxlength=\"64\" autocomplete=\"off\" required>\n      </label>\n      <label>\n        <span>ROM or ZIP</span>\n        <input type=\"file\" name=\"rom\" accept=\".nes,.gb,.gbc,.tap,.ch8,.zip\" required>\n      </label>\n      <button type=\"submit\" class=\"primary-button\">Upload game</button>\n    </form>\n",
        );
        if !palette.is_empty() {
            Self::render_palette(output, csrf, palette);
        }
    }

    fn render_palette(output: &mut String, csrf: &str, palette: &[PaletteField]) {
        output.push_str(
            "    <section class=\"palette-section\">\n      <h2>Dashboard appearance</h2>\n      <form action=\"/palette\" method=\"post\">\n        <input type=\"hidden\" name=\"csrf\" value=\"",
        );
        push_escaped(output, csrf);
        output.push_str(
            "\">\n        <h3>Colors</h3>\n        <p class=\"hint\">#RRGGBB</p>\n        <div class=\"palette-grid\">\n",
        );
        for field in palette {
            output.push_str("          <label>\n            <span>");
            push_escaped(output, field.label);
            output.push_str(
                "</span>\n            <span class=\"palette-inputs\">\n              <input class=\"palette-picker\" type=\"color\" value=\"",
            );
            push_escaped(output, &field.value);
            output.push_str("\" data-palette-picker=\"palette-");
            push_escaped(output, field.name);
            output.push_str("\" aria-label=\"Choose ");
            push_escaped(output, field.label);
            output.push_str("\">\n              <input class=\"palette-hex\" id=\"palette-");
            push_escaped(output, field.name);
            output.push_str("\" type=\"text\" name=\"");
            push_escaped(output, field.name);
            output.push_str("\" value=\"");
            push_escaped(output, &field.value);
            output.push_str(
                "\" pattern=\"#[0-9A-Fa-f]{6}\" minlength=\"7\" maxlength=\"7\" spellcheck=\"false\" autocomplete=\"off\" required>\n            </span>\n          </label>\n",
            );
        }
        output.push_str(
            "        </div>\n        <button type=\"submit\" class=\"primary-button\">Apply appearance</button>\n      </form>\n    </section>\n",
        );
    }
}

fn push_escaped(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
}

/// Paper-style uploader stylesheet served from the same origin.
pub const PAPER_CSS: &str = r#":root {
  color: #000;
  background: #fffefa;
  font-family: "Times New Roman", Times, serif;
  font-size: 18px;
  line-height: 1.4;
}

* { box-sizing: border-box; }

body {
  margin: 0;
  min-width: 320px;
  color: #000;
  background: #fffefa;
}

.skip-link {
  position: fixed;
  top: -100px;
  left: 16px;
  z-index: 10;
  padding: 8px 12px;
  color: #fffefa;
  background: #000;
}

.skip-link:focus { top: 12px; }

main {
  width: min(520px, calc(100% - 40px));
  margin: 0 auto;
  padding: clamp(52px, 10vh, 112px) 0 64px;
}

header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 24px;
  margin-bottom: 40px;
}

h1 {
  margin: 0;
  font-size: clamp(2.4rem, 10vw, 4.2rem);
  line-height: 0.95;
  letter-spacing: -0.04em;
}

h2 {
  margin: 0;
  font-size: 1.7rem;
  line-height: 1;
}

h3 {
  margin: 30px 0 14px;
  font-size: 1.05rem;
  line-height: 1.1;
}

form { margin: 0; }

label {
  display: block;
  margin-bottom: 22px;
}

label > span {
  display: block;
  margin-bottom: 6px;
  font-weight: 700;
}

input, select, button {
  border: 1px solid #000;
  color: #000;
  background: #fffefa;
  font: inherit;
}

input, select {
  width: 100%;
  min-height: 44px;
  padding: 8px 10px;
}

input[type="file"] {
  height: auto;
  padding: 7px;
  font-family: "Courier New", Courier, monospace;
  font-size: 0.8rem;
}

input[type="file"]::file-selector-button {
  margin-right: 10px;
  padding: 6px 9px;
  border: 1px solid #000;
  color: #000;
  background: #fffefa;
  font: inherit;
}

button {
  min-height: 42px;
  padding: 8px 14px;
  cursor: pointer;
}

.primary-button {
  color: #fffefa;
  background: #000;
  font-weight: 700;
}

.quiet-button {
  min-height: 34px;
  padding: 5px 9px;
  font-size: 0.88rem;
}

button:hover, input[type="file"]::file-selector-button:hover {
  color: #fffefa;
  background: #000;
}

.primary-button:hover {
  color: #000;
  background: #fffefa;
}

input:focus, select:focus, button:focus, .skip-link:focus {
  outline: 2px dotted #000;
  outline-offset: 3px;
}

.message {
  margin: -14px 0 28px;
  font-style: italic;
}

.error { font-weight: 700; }

.palette-section { margin-top: 56px; }

.hint {
  margin: 7px 0 20px;
  font-family: "Courier New", Courier, monospace;
  font-size: 0.78rem;
}

.palette-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  column-gap: 20px;
}

.palette-grid label { margin-bottom: 15px; }

.palette-grid label > span {
  overflow-wrap: anywhere;
  font-size: 0.86rem;
}

.palette-inputs {
  display: grid;
  grid-template-columns: 46px minmax(0, 1fr);
  gap: 7px;
}

.palette-grid input { min-height: 44px; }

.palette-picker {
  width: 46px;
  padding: 3px;
}

.palette-hex {
  font-family: "Courier New", Courier, monospace;
  letter-spacing: 0.04em;
}

@media (max-width: 480px) {
  main { width: min(100% - 24px, 520px); }
  header { align-items: flex-start; flex-direction: column; gap: 16px; }
  .palette-grid { grid-template-columns: 1fr; }
}
"#;

/// Color-picker synchronization script served from the same origin.
pub const PALETTE_JS: &str = r#"(function () {
  "use strict";
  var pickers = document.querySelectorAll("[data-palette-picker]");
  for (var index = 0; index < pickers.length; index += 1) {
    (function (picker) {
      var text = document.getElementById(picker.getAttribute("data-palette-picker"));
      if (!text) return;
      picker.addEventListener("input", function () {
        text.value = picker.value.toUpperCase();
      });
      text.addEventListener("input", function () {
        if (/^#[0-9A-Fa-f]{6}$/.test(text.value)) {
          picker.value = text.value;
        }
      });
      text.addEventListener("blur", function () {
        text.value = text.value.toUpperCase();
      });
    }(pickers[index]));
  }
}());
"#;

#[cfg(test)]
mod tests {
    use super::{PALETTE_JS, PAPER_CSS, Page};
    use crate::palette::PaletteField;

    fn fields() -> Vec<PaletteField> {
        vec![PaletteField {
            name: "accent",
            label: "Accent & focus",
            value: "#FE6C27".to_owned(),
        }]
    }

    #[test]
    fn login_page_is_minimal_and_escapes_errors() {
        let page = Page::login(Some("wrong <password> & try \"again\"")).render();
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("wrong &lt;password&gt; &amp; try &quot;again&quot;"));
        assert!(page.contains("action=\"/login\""));
        assert!(!page.contains("action=\"/upload\""));
        assert!(!page.contains("/assets/palette.js"));
        assert!(!page.contains("<hr"));
    }

    #[test]
    fn dashboard_renders_only_rom_palette_and_session_controls() {
        let fields = fields();
        let page =
            Page::dashboard("csrf<&\"", &fields, Some("Game <bad>"), Some("Game & good")).render();
        assert!(page.contains("action=\"/logout\""));
        assert!(page.contains("action=\"/upload\""));
        assert!(page.contains("action=\"/palette\""));
        assert!(page.contains("csrf&lt;&amp;&quot;"));
        assert!(page.contains("Game &lt;bad&gt;"));
        assert!(page.contains("Game &amp; good"));
        assert!(page.contains("Accent &amp; focus"));
        assert!(page.contains("value=\"#FE6C27\""));
        assert!(page.contains("/assets/palette.js"));
        assert!(!page.contains("settings-icon"));
        assert!(!page.contains("Touch a Game"));
        assert!(!page.contains("<hr"));
    }

    #[test]
    fn empty_palette_omits_appearance_controls_and_script() {
        let page = Page::dashboard("csrf", &[], None, None).render();
        assert!(!page.contains("Dashboard appearance"));
        assert!(!page.contains("/assets/palette.js"));
    }

    #[test]
    fn same_origin_assets_keep_the_paper_contract() {
        assert!(PAPER_CSS.contains("Times New Roman"));
        assert!(!PAPER_CSS.contains("border-bottom"));
        assert!(PALETTE_JS.contains("toUpperCase"));
        assert!(!PALETTE_JS.contains("http://"));
        assert!(!PALETTE_JS.contains("https://"));
    }
}
