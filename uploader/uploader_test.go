package main

import (
	"archive/zip"
	"bytes"
	"encoding/hex"
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
	var output bytes.Buffer
	created, err := initializePassword(path, &output)
	if err != nil || !created || !strings.HasPrefix(output.String(), "ROM uploader password: ") {
		t.Fatalf("password initialization failed: created=%v err=%v output=%q", created, err, output.String())
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0600 {
		t.Fatalf("password file is not private: mode=%v", info.Mode())
	}
	before, _ := os.ReadFile(path)
	created, err = initializePassword(path, io.Discard)
	after, _ := os.ReadFile(path)
	if err != nil || created || !bytes.Equal(before, after) {
		t.Fatal("initialization replaced an existing password")
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
	request.Host = serviceAddress
	request.RemoteAddr = "10.0.0.2:41000"
	return request
}

func TestHTTPBoundaryAuthenticationAndUpload(t *testing.T) {
	password := "correct-horse-test-password"
	config := passwordConfig{iterations: 1, salt: []byte("0123456789abcdef")}
	config.digest = derivePassword([]byte(password), config.salt, config.iterations)
	store, root := testStore(t)
	app, err := newApplication(config, store, true)
	if err != nil {
		t.Fatal(err)
	}

	wrongHost := requestFor(http.MethodGet, serviceOrigin+"/", nil)
	wrongHost.Host = "10.0.1.6:8080"
	wrongResponse := httptest.NewRecorder()
	app.ServeHTTP(wrongResponse, wrongHost)
	if wrongResponse.Code != http.StatusMisdirectedRequest {
		t.Fatalf("non-WireGuard host returned %d", wrongResponse.Code)
	}
	nonPeer := requestFor(http.MethodGet, serviceOrigin+"/", nil)
	nonPeer.RemoteAddr = "10.0.1.8:41000"
	nonPeerResponse := httptest.NewRecorder()
	app.ServeHTTP(nonPeerResponse, nonPeer)
	if nonPeerResponse.Code != http.StatusMisdirectedRequest {
		t.Fatalf("non-WireGuard peer returned %d", nonPeerResponse.Code)
	}

	loginBody := url.Values{"password": {password}}.Encode()
	login := requestFor(http.MethodPost, serviceOrigin+"/login", strings.NewReader(loginBody))
	login.Header.Set("Origin", serviceOrigin)
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
	upload := requestFor(http.MethodPost, serviceOrigin+"/upload", &uploadBody)
	upload.Header.Set("Origin", serviceOrigin)
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
}

func TestCrossOriginAndPaperDesignRules(t *testing.T) {
	config := passwordConfig{iterations: 1, salt: []byte("0123456789abcdef"), digest: make([]byte, 32)}
	store, _ := testStore(t)
	app, err := newApplication(config, store, true)
	if err != nil {
		t.Fatal(err)
	}
	request := requestFor(http.MethodPost, serviceOrigin+"/login", strings.NewReader("password=nope"))
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	response := httptest.NewRecorder()
	app.ServeHTTP(response, request)
	if response.Code != http.StatusForbidden {
		t.Fatalf("origin-less login returned %d", response.Code)
	}
	for attempt := 0; attempt < 6; attempt++ {
		failed := requestFor(http.MethodPost, serviceOrigin+"/login", strings.NewReader("password=wrong-password-value"))
		failed.Header.Set("Origin", serviceOrigin)
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
	for _, forbidden := range []string{"rgba(", "color-mix(", "linear-gradient(", "border-radius", "box-shadow", "text-transform", "<hr"} {
		if strings.Contains(paperCSS, forbidden) || strings.Contains(pageTemplate, forbidden) {
			t.Fatalf("Paper hard ban found: %s", forbidden)
		}
	}
}
