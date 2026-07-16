package main

const pageTemplate = `{{define "page"}}<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="robots" content="noindex, nofollow">
  <title>Retro Deck ROM uploader</title>
  <link rel="stylesheet" href="/assets/paper.css">
  <script src="/assets/palette.js" defer></script>
</head>
<body>
  <a class="skip-link" href="#main-content">Skip to content</a>
  <main id="main-content">
    <header>
      <h1>ROM uploader</h1>
      {{if .Authenticated}}
        <form action="/logout" method="post">
          <input type="hidden" name="csrf" value="{{.CSRF}}">
          <button type="submit" class="quiet-button">Sign out</button>
        </form>
      {{end}}
    </header>
    {{if .Authenticated}}
      {{if .Error}}<p class="message error" role="alert"><strong>Not filed:</strong> {{.Error}}</p>{{end}}
      {{if .Notice}}<p class="message" role="status"><strong>Filed:</strong> {{.Notice}}</p>{{end}}
      <form action="/upload" method="post" enctype="multipart/form-data">
        <input type="hidden" name="csrf" value="{{.CSRF}}">
        <label>
          <span>Console</span>
          <select name="system" required>
            <option value="nes">NES</option>
            <option value="gb">Game Boy</option>
            <option value="gbc">Game Boy Color</option>
            <option value="zx">ZX Spectrum</option>
            <option value="chip8">CHIP-8</option>
          </select>
        </label>
        <label>
          <span>Game name</span>
          <input type="text" name="title" minlength="1" maxlength="64" autocomplete="off" required>
        </label>
        <label>
          <span>ROM or ZIP</span>
          <input type="file" name="rom" accept=".nes,.gb,.gbc,.tap,.ch8,.zip" required>
        </label>
        <button type="submit" class="primary-button">Upload game</button>
      </form>
      {{if .Palette}}
        <section class="palette-section">
          <h2>Dashboard appearance</h2>
          <form action="/palette" method="post">
            <input type="hidden" name="csrf" value="{{.CSRF}}">
            <h3 id="settings-icon-label">Settings button</h3>
            <div role="radiogroup" aria-labelledby="settings-icon-label">
              {{range .SettingsIconGroups}}
                <p class="settings-icon-family">{{.Label}}</p>
                <div class="settings-icons">
                  {{range .Icons}}
                    <label class="settings-icon-choice">
                      <input type="radio" name="settings-icon" value="{{.Name}}" {{if .Selected}}checked{{end}} required>
                      <span class="settings-icon-preview">
                        {{if .ImageURL}}
                          <img class="source-cog" src="{{.ImageURL}}" alt="">
                        {{else}}
                          <span class="pixel-cog pixel-cog-{{.GridSize}}" aria-hidden="true">
                            {{range .Pixels}}<b class="{{if .}}on{{end}}"></b>{{end}}
                          </span>
                        {{end}}
                      </span>
                      <span class="settings-icon-name">{{.Label}}</span>
                    </label>
                  {{end}}
                </div>
              {{end}}
            </div>
            <h3>Colors</h3>
            <p class="hint">#RRGGBB</p>
            <div class="palette-grid">
              {{range .Palette}}
                <label>
                  <span>{{.Label}}</span>
                  <span class="palette-inputs">
                    <input class="palette-picker" type="color" value="{{.Value}}" data-palette-picker="palette-{{.Name}}" aria-label="Choose {{.Label}}">
                    <input class="palette-hex" id="palette-{{.Name}}" type="text" name="{{.Name}}" value="{{.Value}}" pattern="#[0-9A-Fa-f]{6}" minlength="7" maxlength="7" spellcheck="false" autocomplete="off" required>
                  </span>
                </label>
              {{end}}
            </div>
            <button type="submit" class="primary-button">Apply appearance</button>
          </form>
        </section>
      {{end}}
    {{else}}
      {{if .Error}}<p class="message error" role="alert"><strong>Error:</strong> {{.Error}}</p>{{end}}
      <form action="/login" method="post">
        <label>
          <span>Password</span>
          <input type="password" name="password" minlength="8" maxlength="128" autocomplete="current-password" required autofocus>
        </label>
        <button type="submit" class="primary-button">Sign in</button>
      </form>
    {{end}}
  </main>
</body>
</html>{{end}}`

const paperCSS = `
:root {
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

.settings-icons {
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
  gap: 8px 6px;
}

.settings-icon-family {
  margin: 18px 0 8px;
  font-family: "Courier New", Courier, monospace;
  font-size: 0.72rem;
  font-weight: 700;
  letter-spacing: 0.08em;
}

.settings-icon-choice {
  position: relative;
  display: grid;
  grid-template-rows: 54px 16px;
  place-items: center;
  gap: 2px;
  min-width: 0;
  min-height: 72px;
  margin: 0;
  cursor: pointer;
}

.settings-icon-choice input {
  position: absolute;
  width: 1px;
  height: 1px;
  opacity: 0;
}

.settings-icon-preview {
  display: grid;
  place-items: center;
  width: 52px;
  height: 52px;
}

.source-cog {
  display: block;
  width: 50px;
  height: 50px;
  object-fit: contain;
  image-rendering: pixelated;
}

.pixel-cog {
  display: grid;
  place-content: center;
  width: 50px;
  height: 50px;
  color: #777;
}

.pixel-cog-9 {
  grid-template-columns: repeat(9, minmax(0, 1fr));
  grid-template-rows: repeat(9, minmax(0, 1fr));
}

.pixel-cog-23 {
  grid-template-columns: repeat(23, minmax(0, 1fr));
  grid-template-rows: repeat(23, minmax(0, 1fr));
}

.pixel-cog b {
  display: block;
  width: 100%;
  height: 100%;
}

.pixel-cog b.on { background: currentColor; }

.settings-icon-name {
  max-width: 100%;
  margin: 0;
  overflow: hidden;
  font-family: "Courier New", Courier, monospace;
  font-size: 0.7rem;
  font-weight: 700;
  line-height: 1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.settings-icon-choice input:checked + .settings-icon-preview {
  outline: 3px solid #000;
  outline-offset: 1px;
}

.settings-icon-choice input:focus-visible + .settings-icon-preview {
  outline: 2px dotted #000;
  outline-offset: 3px;
}

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
  .settings-icons { grid-template-columns: repeat(4, minmax(0, 1fr)); }
  .palette-grid { grid-template-columns: 1fr; }
}
`

const paletteJS = `
(function () {
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
`
