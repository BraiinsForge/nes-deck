package main

const pageTemplate = `{{define "page"}}<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="robots" content="noindex, nofollow">
  <title>Retro Deck ROM intake</title>
  <link rel="stylesheet" href="/assets/paper.css">
</head>
<body>
  <a class="skip-link" href="#main-content">Skip to content</a>
  <header class="shell">
    <div class="letterhead">
      <a class="wordmark" href="/">Retro Deck</a>
      <span class="service-mark">ROM intake</span>
    </div>
    <p class="dateline"><strong>Private service</strong> · WireGuard address 10.0.0.10 · port 8080</p>
  </header>
  <main id="main-content" class="shell">
    {{if .Authenticated}}
      <section class="hero" aria-labelledby="page-title">
        <div class="hero-copy">
          <p class="kicker">Persistent library</p>
          <h1 id="page-title">File a game on the Deck.</h1>
          <p class="lede">Choose the console, give the game its display name, and attach one ROM. A ZIP is accepted only when it contains one matching ROM.</p>
        </div>
        <form class="logout" action="/logout" method="post">
          <input type="hidden" name="csrf" value="{{.CSRF}}">
          <button type="submit" class="button-small">Sign out</button>
        </form>
      </section>
      {{if .Error}}<div class="message message-error" role="alert"><strong>Not filed:</strong> {{.Error}}</div>{{end}}
      {{if .Notice}}<div class="message" role="status"><strong>Filed:</strong> {{.Notice}}</div>{{end}}
      <div class="workspace">
        <section class="window" aria-labelledby="upload-title">
          <div class="window-titlebar" id="upload-title">New catalog entry · Properties</div>
          <div class="window-body">
            <form action="/upload" method="post" enctype="multipart/form-data">
              <input type="hidden" name="csrf" value="{{.CSRF}}">
              <label class="field">
                <span>Console</span>
                <select name="system" required>
                  <option value="nes">NES</option>
                  <option value="gb">Game Boy</option>
                  <option value="gbc">Game Boy Color</option>
                  <option value="zx">ZX Spectrum</option>
                  <option value="chip8">CHIP-8</option>
                </select>
              </label>
              <label class="field">
                <span>Game name</span>
                <input type="text" name="title" minlength="1" maxlength="64" autocomplete="off" required placeholder="Kirby's Adventure">
              </label>
              <label class="field">
                <span>ROM or single-ROM ZIP</span>
                <input type="file" name="rom" accept=".nes,.gb,.gbc,.tap,.ch8,.zip" required>
              </label>
              <p class="helper-note">The file is checked before it reaches the catalog. Existing files are never overwritten.</p>
              <button type="submit" class="button-primary">Validate and file game</button>
            </form>
          </div>
        </section>
        <section class="window" aria-labelledby="catalog-title">
          <div class="window-titlebar" id="catalog-title">Uploaded games · Catalog</div>
          <div class="window-body catalog-body">
            {{if .Entries}}
              <table>
                <thead><tr><th>Game</th><th>Console</th><th>Stored file</th></tr></thead>
                <tbody>{{range .Entries}}<tr><td>{{.Title}}</td><td><code>{{.System}}</code></td><td><code>{{.ROM}}</code></td></tr>{{end}}</tbody>
              </table>
            {{else}}
              <div class="empty-state">
                <p class="empty-title">No web uploads yet.</p>
                <p>Games already shipped with the Deck stay in the main catalog. New validated uploads will be listed here.</p>
              </div>
            {{end}}
          </div>
        </section>
      </div>
    {{else}}
      <section class="hero login-hero" aria-labelledby="page-title">
        <div class="hero-copy">
          <p class="kicker">Persistent library</p>
          <h1 id="page-title">Add games without opening a shell.</h1>
          <p class="lede">This intake desk is reachable only through the Deck's WireGuard tunnel. Sign in to validate and file a ROM.</p>
        </div>
        <div class="window login-window">
          <div class="window-titlebar">ROM intake · Sign in</div>
          <div class="window-body">
            {{if .Error}}<p class="error-note" role="alert"><strong>Error:</strong> {{.Error}}</p>{{end}}
            <form action="/login" method="post">
              <label class="field">
                <span>Password</span>
                <input type="password" name="password" minlength="16" maxlength="128" autocomplete="current-password" required autofocus>
              </label>
              <button type="submit" class="button-primary">Open intake desk</button>
            </form>
          </div>
        </div>
      </section>
      <section class="security-sheet" aria-labelledby="security-title">
        <h2 id="security-title">Connection properties</h2>
        <dl class="property-sheet">
          <dt>Network</dt><dd>WireGuard interface only</dd>
          <dt>Address</dt><dd><code>http://10.0.0.10:8080</code></dd>
          <dt>Uploads</dt><dd>One validated ROM, no replacement</dd>
          <dt>Session</dt><dd>Eight hours, same-site cookie</dd>
        </dl>
      </section>
    {{end}}
  </main>
  <footer class="shell site-footer"><span>Retro Deck local administration</span><span>No public network listener</span></footer>
</body>
</html>{{end}}`

const paperCSS = `
:root {
  color: #000;
  background: #fffefa;
  font-family: "Times New Roman", Times, serif;
  font-size: 17px;
  line-height: 1.45;
}

* { box-sizing: border-box; }

body {
  margin: 0;
  min-width: 320px;
  color: #000;
  background: #fffefa;
}

a { color: #000; text-underline-offset: 0.18em; }
a:hover { text-decoration-thickness: 2px; }

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

.shell {
  width: min(1120px, calc(100% - 40px));
  margin: 0 auto;
}

header.shell { padding-top: 26px; }

.letterhead {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 24px;
  padding-bottom: 8px;
  border-bottom: 2px solid #000;
}

.wordmark {
  font-size: 1.45rem;
  font-weight: 700;
  text-decoration: none;
}

.service-mark { font-style: italic; }

.dateline {
  margin: 7px 0 0;
  font-size: 0.84rem;
}

main.shell { padding: 52px 0 70px; }

.hero {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
  gap: 48px;
  margin-bottom: 38px;
}

.login-hero {
  grid-template-columns: minmax(0, 1.2fr) minmax(300px, 420px);
  align-items: center;
  min-height: 390px;
}

.kicker {
  margin: 0 0 9px;
  font-family: "Courier New", Courier, monospace;
  font-size: 0.78rem;
  letter-spacing: 0.04em;
}

h1, h2, p { margin-top: 0; }

h1 {
  max-width: 760px;
  margin-bottom: 18px;
  font-size: clamp(2.4rem, 7vw, 5.3rem);
  line-height: 0.93;
  letter-spacing: -0.045em;
}

h2 { font-size: 1.5rem; }

.lede {
  max-width: 680px;
  margin-bottom: 0;
  font-size: 1.14rem;
}

.window { border: 1px solid #000; }

.window-titlebar {
  padding: 7px 10px;
  color: #fffefa;
  background: #000;
  font-family: "Courier New", Courier, monospace;
  font-size: 0.83rem;
}

.window-body { padding: 24px; }

.login-window { width: 100%; }

.field {
  display: block;
  margin-bottom: 19px;
}

.field > span {
  display: block;
  margin-bottom: 5px;
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
  min-height: 42px;
  padding: 8px 10px;
}

input[type="file"] {
  height: auto;
  padding: 7px;
  font-family: "Courier New", Courier, monospace;
  font-size: 0.82rem;
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
  padding: 8px 13px;
  cursor: pointer;
}

.button-primary {
  color: #fffefa;
  background: #000;
  font-weight: 700;
}

.button-small {
  min-height: 34px;
  padding: 5px 9px;
  font-size: 0.88rem;
}

button:hover, input[type="file"]::file-selector-button:hover {
  color: #fffefa;
  background: #000;
}

.button-primary:hover {
  color: #000;
  background: #fffefa;
}

input:focus, select:focus, button:focus, a:focus {
  outline: 2px dotted #000;
  outline-offset: 3px;
}

.helper-note, .error-note {
  margin: -8px 0 18px;
  font-size: 0.86rem;
}

.error-note { font-style: italic; }

.logout { margin-top: 6px; }

.message {
  margin: 0 0 26px;
  padding: 12px 14px;
  border: 3px double #000;
}

.message-error { border-style: dashed; }

.workspace {
  display: grid;
  grid-template-columns: minmax(320px, 0.82fr) minmax(0, 1.18fr);
  gap: 28px;
  align-items: start;
}

.catalog-body { overflow-x: auto; }

table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.9rem;
}

th, td {
  padding: 9px 10px;
  border: 1px solid #000;
  text-align: left;
  vertical-align: top;
}

th {
  color: #fffefa;
  background: #000;
}

code {
  font-family: "Courier New", Courier, monospace;
  font-size: 0.82em;
  overflow-wrap: anywhere;
}

.empty-state {
  min-height: 170px;
  display: grid;
  align-content: center;
  justify-items: center;
  padding: 28px;
  border: 1px dashed #000;
  text-align: center;
}

.empty-state p { max-width: 390px; }
.empty-title { margin-bottom: 7px; font-size: 1.2rem; font-weight: 700; }

.security-sheet {
  max-width: 720px;
  margin-top: 18px;
}

.property-sheet {
  display: grid;
  grid-template-columns: 140px minmax(0, 1fr);
  margin: 0;
  border: 1px solid #000;
}

.property-sheet dt, .property-sheet dd {
  margin: 0;
  padding: 8px 10px;
  border-bottom: 1px solid #000;
}

.property-sheet dt { font-weight: 700; }
.property-sheet dt:nth-last-of-type(1), .property-sheet dd:nth-last-of-type(1) { border-bottom: 0; }

.site-footer {
  display: flex;
  justify-content: space-between;
  gap: 20px;
  padding: 18px 0 28px;
  border-top: 2px solid #000;
  font-size: 0.82rem;
}

@media (max-width: 800px) {
  .hero, .login-hero, .workspace { grid-template-columns: 1fr; }
  .login-hero { min-height: 0; }
  .login-window { max-width: 520px; }
  h1 { font-size: clamp(2.6rem, 13vw, 4.6rem); }
}

@media (max-width: 520px) {
  .shell { width: min(100% - 24px, 1120px); }
  header.shell { padding-top: 16px; }
  .letterhead, .site-footer { align-items: flex-start; flex-direction: column; gap: 5px; }
  main.shell { padding-top: 34px; }
  .window-body { padding: 18px; }
  .property-sheet { grid-template-columns: 1fr; }
  .property-sheet dt { padding-bottom: 2px; border-bottom: 0; }
  .property-sheet dd { padding-top: 2px; }
}
`
