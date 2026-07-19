package main

import (
	"archive/zip"
	"bytes"
	"encoding/hex"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const (
	testListenAddress  = "0.0.0.0:8080"
	testServiceAddress = "10.0.0.10:8080"
	testServiceOrigin  = "http://" + testServiceAddress
)

func testNES() []byte {
	rom := make([]byte, 16+16384)
	copy(rom, []byte{'N', 'E', 'S', 0x1a})
	rom[4] = 1
	return rom
}

func zipMembers(t *testing.T, members map[string][]byte) []byte {
	t.Helper()
	var output bytes.Buffer
	writer := zip.NewWriter(&output)
	for name, contents := range members {
		member, err := writer.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := member.Write(contents); err != nil {
			t.Fatal(err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	return output.Bytes()
}

func TestPasswordDerivationAndConfiguration(t *testing.T) {
	expected, _ := hex.DecodeString("120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b")
	derived := derivePassword([]byte("password"), []byte("salt"), 1)
	if !bytes.Equal(derived, expected) {
		t.Fatalf("PBKDF2 vector mismatch: %x", derived)
	}
	config, err := newPasswordConfig("a-long-test-password")
	if err != nil {
		t.Fatal(err)
	}
	if !config.matches("a-long-test-password") || config.matches("the-wrong-password") {
		t.Fatal("password comparison accepted the wrong value")
	}
	parsed, err := parsePasswordConfig(encodePasswordConfig(config))
	if err != nil || !parsed.matches("a-long-test-password") {
		t.Fatalf("password configuration did not round-trip: %v", err)
	}

	directory := t.TempDir()
	path := filepath.Join(directory, "private", "password.conf")
	if err := atomicWrite(path, encodePasswordConfig(config), 0600); err != nil {
		t.Fatalf("password configuration write failed: %v", err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0600 {
		t.Fatalf("password file is not private: mode=%v", info.Mode())
	}
	loaded, err := loadPasswordConfig(path)
	if err != nil || !loaded.matches("a-long-test-password") {
		t.Fatalf("installed password configuration did not load: %v", err)
	}
}

func TestPasswordInputValidation(t *testing.T) {
	password, err := readPassword(strings.NewReader("configured-test-password\n"))
	if err != nil || password != "configured-test-password" {
		t.Fatalf("configured password was rejected: %q %v", password, err)
	}
	for _, input := range []string{"short\n", strings.Repeat("x", maximumPasswordSize+1) + "\n", "valid-password-value\rjunk\n"} {
		if _, err := readPassword(strings.NewReader(input)); err == nil {
			t.Fatalf("invalid password input was accepted: %q", input)
		}
	}
}

func TestServiceAddressConfiguration(t *testing.T) {
	if normalized, err := normalizeServiceAddress(testListenAddress); err != nil || normalized != testListenAddress {
		t.Fatalf("all-interface service address was rejected: %q %v", normalized, err)
	}
	for _, address := range []string{"", "10.0.0.10:8080", "0.0.0.0:80", "localhost:8080", "[::]:8080"} {
		if _, err := normalizeServiceAddress(address); err == nil {
			t.Fatalf("invalid service address was accepted: %q", address)
		}
	}
	directory := t.TempDir()
	path := filepath.Join(directory, "address.conf")
	if err := os.WriteFile(path, []byte(testListenAddress+"\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if address, err := loadServiceAddress(path); err != nil || address != testListenAddress {
		t.Fatalf("service address configuration did not load: %q %v", address, err)
	}
}

func TestROMValidationAndZIPBoundaries(t *testing.T) {
	rom := testNES()
	if err := validateROM("nes", rom); err != nil {
		t.Fatal(err)
	}
	if _, err := decodeUpload("nes", "game.nes", bytes.NewReader(rom)); err != nil {
		t.Fatal(err)
	}
	archive := zipMembers(t, map[string][]byte{"game.nes": rom})
	if extracted, err := decodeUpload("nes", "game.zip", bytes.NewReader(archive)); err != nil || !bytes.Equal(extracted, rom) {
		t.Fatalf("single-ROM ZIP was rejected: %v", err)
	}
	multiple := zipMembers(t, map[string][]byte{"one.nes": rom, "two.nes": rom})
	if _, err := decodeUpload("nes", "games.zip", bytes.NewReader(multiple)); err == nil {
		t.Fatal("multi-ROM ZIP was accepted")
	}
	unsafe := zipMembers(t, map[string][]byte{"../game.nes": rom})
	if _, err := decodeUpload("nes", "unsafe.zip", bytes.NewReader(unsafe)); err == nil {
		t.Fatal("path-bearing ZIP member was accepted")
	}
	if _, err := decodeUpload("gb", "game.gb", bytes.NewReader(rom)); err == nil {
		t.Fatal("NES data was accepted as Game Boy data")
	}
}

func testStore(t *testing.T) (*romStore, string) {
	t.Helper()
	directory := t.TempDir()
	base := filepath.Join(directory, "base.tsv")
	if err := os.WriteFile(base, nil, 0600); err != nil {
		t.Fatal(err)
	}
	root := filepath.Join(directory, "roms")
	store := &romStore{
		romRoot:       root,
		baseCatalog:   base,
		uploadCatalog: filepath.Join(directory, "uploads", "games.tsv"),
	}
	return store, root
}

func testPalette(t *testing.T) (*paletteStore, string) {
	t.Helper()
	directory := t.TempDir()
	base := filepath.Join(directory, "palette.tsv")
	var contents strings.Builder
	for index, spec := range dashboardPaletteSpecs {
		contents.WriteString(spec.name)
		contents.WriteByte('\t')
		contents.WriteString(testRGB(index))
		contents.WriteByte('\n')
	}
	if err := os.WriteFile(base, []byte(contents.String()), 0600); err != nil {
		t.Fatal(err)
	}
	override := filepath.Join(directory, "state", "dashboard-palette.sexp")
	return &paletteStore{
		activePath:   filepath.Join(directory, "missing-active.tsv"),
		fallbackPath: base,
		overridePath: override,
	}, override
}

func testRGB(index int) string {
	return fmt.Sprintf("#%02X%02X%02X", index*3+1, index*3+2, index*3+3)
}

func TestDashboardPaletteConfiguration(t *testing.T) {
	store, overridePath := testPalette(t)
	fields, icons, err := store.current()
	if err != nil || len(fields) != len(dashboardPaletteSpecs) || fields[0].Value != "#010203" || len(icons) != 48 || !icons[20].Selected {
		t.Fatalf("fallback palette did not load: %#v %v", fields, err)
	}
	knekkoIcons := 0
	for _, spec := range settingsIconSpecs {
		if spec.imageURL != "" {
			if len(spec.rows) != 0 || !strings.HasPrefix(spec.imageURL, "/assets/settings-icons/") {
				t.Fatalf("source settings icon %s has invalid asset metadata", spec.name)
			}
			knekkoIcons++
			continue
		}
		if len(spec.rows) != 9 && len(spec.rows) != 23 {
			t.Fatalf("settings icon %s uses unsupported grid size %d", spec.name, len(spec.rows))
		}
		for _, row := range spec.rows {
			if len(row) != len(spec.rows) {
				t.Fatalf("settings icon %s is not square", spec.name)
			}
		}
	}
	groups := settingsIconGroups(icons)
	if knekkoIcons != 36 || len(groups) != 4 || len(groups[1].Icons) != 6 || len(groups[2].Icons) != 10 || len(groups[3].Icons) != 20 {
		t.Fatalf("complete source cog set was not grouped: %#v", groups)
	}
	var stale strings.Builder
	for index, spec := range dashboardPaletteSpecs {
		stale.WriteString(spec.name)
		stale.WriteByte('\t')
		stale.WriteString(testRGB(index + 32))
		stale.WriteByte('\n')
	}
	if err := os.WriteFile(store.activePath, []byte(stale.String()), 0600); err != nil {
		t.Fatal(err)
	}
	fields, icons, err = store.current()
	if err != nil || fields[0].Value != "#010203" {
		t.Fatalf("stale generated palette overrode the checked-in fallback: %#v %v", fields, err)
	}
	values := make(map[string]string, len(fields))
	for _, field := range fields {
		values[field.Name] = field.Value
	}
	values["accent"] = "#123456"
	restarts := 0
	store.restartDashboard = func() error {
		restarts++
		return nil
	}
	if err := store.save(values, "gear-rivet"); err != nil {
		t.Fatal(err)
	}
	if restarts != 1 {
		t.Fatalf("palette save restarted dashboard %d times", restarts)
	}
	contents, err := os.ReadFile(overridePath)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := parsePaletteOverride(contents)
	if err != nil || parsed.palette["accent"] != "#123456" || parsed.settingsIcon != "gear-rivet" {
		t.Fatalf("palette override did not round-trip: %#v %v", parsed, err)
	}
	fields, icons, err = store.current()
	if err != nil {
		t.Fatal(err)
	}
	for _, field := range fields {
		if field.Name == "accent" && field.Value != "#123456" {
			t.Fatalf("valid override was not displayed: %#v", fields)
		}
	}
	if !icons[9].Selected {
		t.Fatalf("valid settings icon was not displayed: %#v", icons)
	}
	if err := os.WriteFile(overridePath, []byte("(:version 2 :palette (:background \"#12345G\"))\n"), 0600); err != nil {
		t.Fatal(err)
	}
	fields, icons, err = store.current()
	if err != nil || fields[0].Value != "#010203" {
		t.Fatalf("bad optional override hid the fallback palette: %#v %v", fields, err)
	}
	values["accent"] = "#12345G"
	if err := store.save(values, "gear-rivet"); err == nil {
		t.Fatal("malformed palette value was accepted")
	}
	values["accent"] = "#123456"
	if err := store.save(values, "not-a-cog"); err == nil {
		t.Fatal("unknown settings icon was accepted")
	}
}

func TestROMStoreFilesWithoutReplacement(t *testing.T) {
	store, root := testStore(t)
	entry, err := store.add("nes", "Test Game", "source.nes", bytes.NewReader(testNES()))
	if err != nil {
		t.Fatal(err)
	}
	expected := filepath.Join(root, "nes", "test-game.nes")
	if entry.ID != "upload-nes-test-game" || entry.ROM != expected {
		t.Fatalf("unexpected catalog entry: %#v", entry)
	}
	if contents, err := os.ReadFile(expected); err != nil || !bytes.Equal(contents, testNES()) {
		t.Fatalf("ROM was not installed intact: %v", err)
	}
	entries, err := parseCatalog(store.uploadCatalog, false)
	if err != nil || len(entries) != 1 || entries[0].Title != "Test Game" {
		t.Fatalf("upload catalog mismatch: %#v %v", entries, err)
	}
	if _, err := store.add("nes", "Test Game", "again.nes", bytes.NewReader(testNES())); err == nil {
		t.Fatal("duplicate upload replaced an existing game")
	}
}

func requestFor(method, target string, body io.Reader) *http.Request {
	request := httptest.NewRequest(method, target, body)
	request.Host = testServiceAddress
	request.RemoteAddr = "10.0.0.2:41000"
	return request
}

func TestHTTPBoundaryAuthenticationAndUpload(t *testing.T) {
	password := "correct-horse-test-password"
	config := passwordConfig{iterations: 1, salt: []byte("0123456789abcdef")}
	config.digest = derivePassword([]byte(password), config.salt, config.iterations)
	store, root := testStore(t)
	palette, overridePath := testPalette(t)
	paletteRestarts := 0
	palette.restartDashboard = func() error {
		paletteRestarts++
		return nil
	}
	app, err := newApplication(config, store, palette, testListenAddress)
	if err != nil {
		t.Fatal(err)
	}

	wrongHost := requestFor(http.MethodGet, testServiceOrigin+"/", nil)
	wrongHost.Host = "deck.local:8080"
	wrongResponse := httptest.NewRecorder()
	app.ServeHTTP(wrongResponse, wrongHost)
	if wrongResponse.Code != http.StatusMisdirectedRequest {
		t.Fatalf("non-IP host returned %d", wrongResponse.Code)
	}
	wifiRequest := requestFor(http.MethodGet, "http://192.168.1.20:8080/", nil)
	wifiRequest.Host = "192.168.1.20:8080"
	wifiRequest.RemoteAddr = "192.168.1.50:41000"
	wifiResponse := httptest.NewRecorder()
	app.ServeHTTP(wifiResponse, wifiRequest)
	if wifiResponse.Code != http.StatusOK {
		t.Fatalf("Wi-Fi interface request returned %d", wifiResponse.Code)
	}

	loginBody := url.Values{"password": {password}}.Encode()
	login := requestFor(http.MethodPost, testServiceOrigin+"/login", strings.NewReader(loginBody))
	login.Header.Set("Origin", testServiceOrigin)
	login.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	loginResponse := httptest.NewRecorder()
	app.ServeHTTP(loginResponse, login)
	if loginResponse.Code != http.StatusSeeOther {
		t.Fatalf("login returned %d: %s", loginResponse.Code, loginResponse.Body.String())
	}
	cookies := loginResponse.Result().Cookies()
	if len(cookies) != 1 || !cookies[0].HttpOnly || cookies[0].SameSite != http.SameSiteStrictMode {
		t.Fatalf("session cookie is not hardened: %#v", cookies)
	}
	app.mu.Lock()
	session := app.sessions[sessionKey(cookies[0].Value)]
	app.mu.Unlock()

	var uploadBody bytes.Buffer
	multipartWriter := multipart.NewWriter(&uploadBody)
	_ = multipartWriter.WriteField("csrf", session.csrf)
	_ = multipartWriter.WriteField("system", "nes")
	_ = multipartWriter.WriteField("title", "Web Game")
	fileWriter, err := multipartWriter.CreateFormFile("rom", "web-game.nes")
	if err != nil {
		t.Fatal(err)
	}
	_, _ = fileWriter.Write(testNES())
	_ = multipartWriter.Close()
	upload := requestFor(http.MethodPost, testServiceOrigin+"/upload", &uploadBody)
	upload.Header.Set("Origin", testServiceOrigin)
	upload.Header.Set("Content-Type", multipartWriter.FormDataContentType())
	upload.AddCookie(cookies[0])
	uploadResponse := httptest.NewRecorder()
	app.ServeHTTP(uploadResponse, upload)
	if uploadResponse.Code != http.StatusOK || !strings.Contains(uploadResponse.Body.String(), "was validated, filed") {
		t.Fatalf("upload returned %d: %s", uploadResponse.Code, uploadResponse.Body.String())
	}
	if _, err := os.Stat(filepath.Join(root, "nes", "web-game.nes")); err != nil {
		t.Fatalf("web upload did not reach storage: %v", err)
	}
	if uploadResponse.Header().Get("Content-Security-Policy") == "" || uploadResponse.Header().Get("X-Frame-Options") != "DENY" {
		t.Fatal("security headers are missing")
	}

	paletteForm := url.Values{"csrf": {session.csrf}}
	for index, spec := range dashboardPaletteSpecs {
		paletteForm.Set(spec.name, strings.ToLower(testRGB(index+32)))
	}
	paletteRequest := requestFor(http.MethodPost, testServiceOrigin+"/palette", strings.NewReader(paletteForm.Encode()))
	paletteRequest.Header.Set("Origin", testServiceOrigin)
	paletteRequest.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	paletteRequest.AddCookie(cookies[0])
	paletteResponse := httptest.NewRecorder()
	app.ServeHTTP(paletteResponse, paletteRequest)
	if paletteResponse.Code != http.StatusOK || !strings.Contains(paletteResponse.Body.String(), "saved and applied") {
		t.Fatalf("palette update returned %d: %s", paletteResponse.Code, paletteResponse.Body.String())
	}
	if !strings.Contains(paletteResponse.Body.String(), `type="color"`) ||
		!strings.Contains(paletteResponse.Body.String(), `value="#616263"`) ||
		strings.Contains(paletteResponse.Body.String(), `name="settings-icon"`) ||
		!strings.Contains(paletteResponse.Body.String(), `/assets/palette.js`) {
		t.Fatal("appearance response does not expose only the RGB controls")
	}
	if paletteRestarts != 1 {
		t.Fatalf("palette update restarted dashboard %d times", paletteRestarts)
	}
	overrideContents, err := os.ReadFile(overridePath)
	if err != nil {
		t.Fatalf("palette update was not persisted: %v", err)
	}
	installedPalette, err := parsePaletteOverride(overrideContents)
	if err != nil || installedPalette.palette[dashboardPaletteSpecs[0].name] != "#616263" || installedPalette.settingsIcon != defaultSettingsIcon {
		t.Fatalf("HTTP palette was not normalized and persisted: %#v %v", installedPalette, err)
	}

	scriptRequest := requestFor(http.MethodGet, testServiceOrigin+"/assets/palette.js", nil)
	scriptResponse := httptest.NewRecorder()
	app.ServeHTTP(scriptResponse, scriptRequest)
	if scriptResponse.Code != http.StatusOK ||
		!strings.Contains(scriptResponse.Body.String(), "toUpperCase") ||
		scriptResponse.Header().Get("Content-Type") != "text/javascript; charset=utf-8" {
		t.Fatalf("palette synchronization asset returned %d", scriptResponse.Code)
	}

	iconRequest := requestFor(http.MethodGet, testServiceOrigin+"/assets/settings-icons/01.png", nil)
	iconResponse := httptest.NewRecorder()
	app.ServeHTTP(iconResponse, iconRequest)
	if iconResponse.Code != http.StatusOK ||
		!strings.HasPrefix(iconResponse.Header().Get("Content-Type"), "image/png") ||
		iconResponse.Header().Get("Cache-Control") != "public, max-age=31536000, immutable" ||
		!strings.Contains(iconResponse.Header().Get("Content-Security-Policy"), "img-src 'self'") ||
		!bytes.HasPrefix(iconResponse.Body.Bytes(), []byte("\x89PNG\r\n\x1a\n")) {
		t.Fatalf("embedded settings icon returned %d", iconResponse.Code)
	}
	unknownIconRequest := requestFor(http.MethodGet, testServiceOrigin+"/assets/settings-icons/../UPSTREAM.txt", nil)
	unknownIconResponse := httptest.NewRecorder()
	app.ServeHTTP(unknownIconResponse, unknownIconRequest)
	if unknownIconResponse.Code != http.StatusNotFound {
		t.Fatalf("unknown settings asset returned %d", unknownIconResponse.Code)
	}
}

func TestCrossOriginAndPaperDesignRules(t *testing.T) {
	config := passwordConfig{iterations: 1, salt: []byte("0123456789abcdef"), digest: make([]byte, 32)}
	store, _ := testStore(t)
	palette, _ := testPalette(t)
	app, err := newApplication(config, store, palette, testListenAddress)
	if err != nil {
		t.Fatal(err)
	}
	for _, origin := range []string{"", "null", testServiceOrigin} {
		request := requestFor(http.MethodPost, testServiceOrigin+"/login", nil)
		request.Header.Set("Origin", origin)
		if !app.sameOrigin(request) {
			t.Fatalf("browser-compatible origin was rejected: %q", origin)
		}
	}
	wifiOrigin := "http://192.168.1.20:8080"
	wifiRequest := requestFor(http.MethodPost, wifiOrigin+"/login", nil)
	wifiRequest.Host = "192.168.1.20:8080"
	wifiRequest.Header.Set("Origin", wifiOrigin)
	if !app.sameOrigin(wifiRequest) {
		t.Fatal("same-origin policy rejected the Wi-Fi interface origin")
	}
	wifiRequest.Header.Set("Origin", testServiceOrigin)
	if app.sameOrigin(wifiRequest) {
		t.Fatal("same-origin policy accepted an origin from another interface")
	}
	foreign := requestFor(http.MethodPost, testServiceOrigin+"/login", strings.NewReader("password=nope"))
	foreign.Header.Set("Origin", "http://example.com")
	foreign.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	foreignResponse := httptest.NewRecorder()
	app.ServeHTTP(foreignResponse, foreign)
	if foreignResponse.Code != http.StatusForbidden {
		t.Fatalf("foreign-origin login returned %d", foreignResponse.Code)
	}
	for attempt := 0; attempt < 6; attempt++ {
		failed := requestFor(http.MethodPost, testServiceOrigin+"/login", strings.NewReader("password=wrong-password-value"))
		failed.Header.Set("Origin", testServiceOrigin)
		failed.Header.Set("Content-Type", "application/x-www-form-urlencoded")
		failedResponse := httptest.NewRecorder()
		app.ServeHTTP(failedResponse, failed)
		if attempt < 5 && failedResponse.Code != http.StatusUnauthorized {
			t.Fatalf("failed login %d returned %d", attempt+1, failedResponse.Code)
		}
		if attempt == 5 && failedResponse.Code != http.StatusTooManyRequests {
			t.Fatalf("locked login returned %d", failedResponse.Code)
		}
	}
	for _, forbidden := range []string{
		"rgba(", "color-mix(", "linear-gradient(", "border-radius", "box-shadow", "text-transform",
		"<hr", "border-top", "border-bottom", "Private service", "Connection properties",
		"No public network listener", "Persistent library", "Uploaded games", "<table",
	} {
		if strings.Contains(paperCSS, forbidden) || strings.Contains(pageTemplate, forbidden) {
			t.Fatalf("Paper hard ban found: %s", forbidden)
		}
	}
}
