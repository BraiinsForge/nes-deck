package main

const pageTemplate = `{{define "page"}}<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="robots" content="noindex, nofollow">
  <title>Retro Deck ROM uploader</title>
  <link rel="stylesheet" href="/assets/paper.css">
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

@media (max-width: 480px) {
  main { width: min(100% - 24px, 520px); }
  header { align-items: flex-start; flex-direction: column; gap: 16px; }
}
`
