package main

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"html/template"
	"log"
	"mime"
	"net"
	"net/http"
	"os/exec"
	"strings"
	"sync"
	"syscall"
	"time"
)

const (
	serviceAddress      = "10.0.0.10:8080"
	serviceInterface    = "wg0"
	serviceOrigin       = "http://10.0.0.10:8080"
	sessionCookieName   = "deck_rom_session"
	sessionLifetime     = 8 * time.Hour
	maximumSessions     = 8
	maximumLoginSources = 256
)

type userSession struct {
	csrf      string
	expiresAt time.Time
}

type loginAttempt struct {
	failures    int
	lockedUntil time.Time
	lastSeen    time.Time
}

type pageData struct {
	Authenticated bool
	CSRF          string
	Error         string
	Notice        string
	Entries       []catalogEntry
}

type application struct {
	password       passwordConfig
	store          *romStore
	origin         string
	host           string
	enforceNetwork bool
	now            func() time.Time
	template       *template.Template

	mu        sync.Mutex
	sessions  map[string]userSession
	attempts  map[string]loginAttempt
	loginGate chan struct{}
}

func newApplication(password passwordConfig, store *romStore, enforceNetwork bool) (*application, error) {
	parsed, err := template.New("page").Parse(pageTemplate)
	if err != nil {
		return nil, err
	}
	return &application{
		password:       password,
		store:          store,
		origin:         serviceOrigin,
		host:           serviceAddress,
		enforceNetwork: enforceNetwork,
		now:            time.Now,
		template:       parsed,
		sessions:       make(map[string]userSession),
		attempts:       make(map[string]loginAttempt),
		loginGate:      make(chan struct{}, 1),
	}, nil
}

func randomToken(size int) (string, error) {
	value := make([]byte, size)
	if _, err := rand.Read(value); err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(value), nil
}

func sessionKey(token string) string {
	digest := sha256.Sum256([]byte(token))
	return hex.EncodeToString(digest[:])
}

func remoteHost(remoteAddress string) string {
	host, _, err := net.SplitHostPort(remoteAddress)
	if err != nil {
		return ""
	}
	return host
}

func wireGuardPeer(host string) bool {
	address := net.ParseIP(host).To4()
	return address != nil && address[0] == 10 && address[1] == 0 && address[2] == 0
}

func (app *application) securityHeaders(response http.ResponseWriter) {
	response.Header().Set("Cache-Control", "no-store")
	response.Header().Set("Content-Security-Policy", "default-src 'none'; style-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'")
	response.Header().Set("Cross-Origin-Opener-Policy", "same-origin")
	response.Header().Set("Cross-Origin-Resource-Policy", "same-origin")
	response.Header().Set("Referrer-Policy", "no-referrer")
	response.Header().Set("X-Content-Type-Options", "nosniff")
	response.Header().Set("X-Frame-Options", "DENY")
}

func (app *application) requestAllowed(request *http.Request) bool {
	if request.Host != app.host {
		return false
	}
	return !app.enforceNetwork || wireGuardPeer(remoteHost(request.RemoteAddr))
}

func (app *application) sameOrigin(request *http.Request) bool {
	return request.Header.Get("Origin") == app.origin
}

func (app *application) currentSession(request *http.Request) (string, userSession, bool) {
	cookie, err := request.Cookie(sessionCookieName)
	if err != nil || len(cookie.Value) != 43 {
		return "", userSession{}, false
	}
	key := sessionKey(cookie.Value)
	now := app.now()
	app.mu.Lock()
	defer app.mu.Unlock()
	session, ok := app.sessions[key]
	if !ok || !session.expiresAt.After(now) {
		delete(app.sessions, key)
		return "", userSession{}, false
	}
	return key, session, true
}

func (app *application) createSession(response http.ResponseWriter) (userSession, error) {
	token, err := randomToken(32)
	if err != nil {
		return userSession{}, err
	}
	csrf, err := randomToken(32)
	if err != nil {
		return userSession{}, err
	}
	now := app.now()
	session := userSession{csrf: csrf, expiresAt: now.Add(sessionLifetime)}
	app.mu.Lock()
	for key, candidate := range app.sessions {
		if !candidate.expiresAt.After(now) {
			delete(app.sessions, key)
		}
	}
	if len(app.sessions) >= maximumSessions {
		var oldestKey string
		var oldest time.Time
		for key, candidate := range app.sessions {
			if oldestKey == "" || candidate.expiresAt.Before(oldest) {
				oldestKey = key
				oldest = candidate.expiresAt
			}
		}
		delete(app.sessions, oldestKey)
	}
	app.sessions[sessionKey(token)] = session
	app.mu.Unlock()
	http.SetCookie(response, &http.Cookie{
		Name:     sessionCookieName,
		Value:    token,
		Path:     "/",
		HttpOnly: true,
		SameSite: http.SameSiteStrictMode,
		MaxAge:   int(sessionLifetime.Seconds()),
	})
	return session, nil
}

func (app *application) csrfValid(request *http.Request, session userSession) bool {
	provided := request.FormValue("csrf")
	return len(provided) == len(session.csrf) && subtle.ConstantTimeCompare([]byte(provided), []byte(session.csrf)) == 1
}

func (app *application) loginBlocked(host string) (bool, time.Duration) {
	now := app.now()
	app.mu.Lock()
	defer app.mu.Unlock()
	attempt := app.attempts[host]
	if attempt.lockedUntil.After(now) {
		return true, attempt.lockedUntil.Sub(now)
	}
	return false, 0
}

func (app *application) recordLogin(host string, successful bool) {
	now := app.now()
	app.mu.Lock()
	defer app.mu.Unlock()
	if successful {
		delete(app.attempts, host)
		return
	}
	if len(app.attempts) >= maximumLoginSources {
		var oldestHost string
		var oldest time.Time
		for candidateHost, attempt := range app.attempts {
			if oldestHost == "" || attempt.lastSeen.Before(oldest) {
				oldestHost = candidateHost
				oldest = attempt.lastSeen
			}
		}
		delete(app.attempts, oldestHost)
	}
	attempt := app.attempts[host]
	attempt.failures++
	attempt.lastSeen = now
	if attempt.failures >= 5 {
		attempt.failures = 0
		attempt.lockedUntil = now.Add(5 * time.Minute)
	}
	app.attempts[host] = attempt
}

func (app *application) render(response http.ResponseWriter, status int, data pageData) {
	response.Header().Set("Content-Type", "text/html; charset=utf-8")
	response.WriteHeader(status)
	if err := app.template.ExecuteTemplate(response, "page", data); err != nil {
		log.Printf("render page: %v", err)
	}
}

func (app *application) dashboardData(session userSession, message, notice string) pageData {
	entries, err := app.store.entries()
	if err != nil && message == "" {
		message = "The upload catalog cannot be read."
	}
	return pageData{Authenticated: true, CSRF: session.csrf, Error: message, Notice: notice, Entries: entries}
}

func (app *application) handleIndex(response http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodGet {
		response.Header().Set("Allow", http.MethodGet)
		http.Error(response, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}
	_, session, ok := app.currentSession(request)
	if !ok {
		app.render(response, http.StatusOK, pageData{})
		return
	}
	app.render(response, http.StatusOK, app.dashboardData(session, "", ""))
}

func (app *application) handleLogin(response http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodPost {
		response.Header().Set("Allow", http.MethodPost)
		http.Error(response, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if !app.sameOrigin(request) {
		http.Error(response, "Cross-origin request rejected", http.StatusForbidden)
		return
	}
	mediaType, _, err := mime.ParseMediaType(request.Header.Get("Content-Type"))
	if err != nil || mediaType != "application/x-www-form-urlencoded" {
		http.Error(response, "Unsupported form encoding", http.StatusUnsupportedMediaType)
		return
	}
	request.Body = http.MaxBytesReader(response, request.Body, 512)
	if err := request.ParseForm(); err != nil {
		app.render(response, http.StatusBadRequest, pageData{Error: "The login form was malformed."})
		return
	}
	host := remoteHost(request.RemoteAddr)
	if blocked, remaining := app.loginBlocked(host); blocked {
		response.Header().Set("Retry-After", fmt.Sprintf("%d", int(remaining.Seconds())+1))
		app.render(response, http.StatusTooManyRequests, pageData{Error: "Too many attempts. Wait five minutes before trying again."})
		return
	}
	select {
	case app.loginGate <- struct{}{}:
		defer func() { <-app.loginGate }()
	case <-request.Context().Done():
		return
	case <-time.After(2 * time.Second):
		response.Header().Set("Retry-After", "3")
		app.render(response, http.StatusTooManyRequests, pageData{Error: "Another sign-in is being checked. Try again in a moment."})
		return
	}
	if blocked, remaining := app.loginBlocked(host); blocked {
		response.Header().Set("Retry-After", fmt.Sprintf("%d", int(remaining.Seconds())+1))
		app.render(response, http.StatusTooManyRequests, pageData{Error: "Too many attempts. Wait five minutes before trying again."})
		return
	}
	successful := app.password.matches(request.Form.Get("password"))
	app.recordLogin(host, successful)
	if !successful {
		app.render(response, http.StatusUnauthorized, pageData{Error: "That password was not accepted."})
		return
	}
	if _, err := app.createSession(response); err != nil {
		http.Error(response, "Session creation failed", http.StatusInternalServerError)
		return
	}
	http.Redirect(response, request, "/", http.StatusSeeOther)
}

func (app *application) handleLogout(response http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodPost || !app.sameOrigin(request) {
		http.Error(response, "Request rejected", http.StatusForbidden)
		return
	}
	key, session, ok := app.currentSession(request)
	if !ok || !app.csrfValid(request, session) {
		http.Error(response, "Request rejected", http.StatusForbidden)
		return
	}
	app.mu.Lock()
	delete(app.sessions, key)
	app.mu.Unlock()
	http.SetCookie(response, &http.Cookie{Name: sessionCookieName, Value: "", Path: "/", HttpOnly: true, SameSite: http.SameSiteStrictMode, MaxAge: -1})
	http.Redirect(response, request, "/", http.StatusSeeOther)
}

func (app *application) handleUpload(response http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodPost || !app.sameOrigin(request) {
		http.Error(response, "Request rejected", http.StatusForbidden)
		return
	}
	_, session, ok := app.currentSession(request)
	if !ok {
		http.Error(response, "Authentication required", http.StatusUnauthorized)
		return
	}
	request.Body = http.MaxBytesReader(response, request.Body, maximumRequestBytes)
	if err := request.ParseMultipartForm(1024 * 1024); err != nil {
		app.render(response, http.StatusBadRequest, app.dashboardData(session, "The upload form was malformed or too large.", ""))
		return
	}
	defer request.MultipartForm.RemoveAll()
	if !app.csrfValid(request, session) {
		http.Error(response, "Request rejected", http.StatusForbidden)
		return
	}
	files := request.MultipartForm.File["rom"]
	if len(files) != 1 {
		app.render(response, http.StatusBadRequest, app.dashboardData(session, "Choose exactly one ROM or single-ROM ZIP.", ""))
		return
	}
	file, err := files[0].Open()
	if err != nil {
		app.render(response, http.StatusBadRequest, app.dashboardData(session, "The uploaded file could not be opened.", ""))
		return
	}
	defer file.Close()
	entry, addErr := app.store.add(request.FormValue("system"), request.FormValue("title"), files[0].Filename, file)
	if addErr != nil && entry.ID == "" {
		app.render(response, http.StatusUnprocessableEntity, app.dashboardData(session, addErr.Error(), ""))
		return
	}
	notice := fmt.Sprintf("%s was validated, filed, and added to the dashboard.", entry.Title)
	if addErr != nil {
		notice = fmt.Sprintf("%s was saved. The dashboard will pick it up after its next restart.", entry.Title)
	}
	app.render(response, http.StatusOK, app.dashboardData(session, "", notice))
}

func (app *application) ServeHTTP(response http.ResponseWriter, request *http.Request) {
	app.securityHeaders(response)
	if !app.requestAllowed(request) {
		http.Error(response, "This service is available only through its WireGuard address.", http.StatusMisdirectedRequest)
		return
	}
	switch request.URL.Path {
	case "/":
		app.handleIndex(response, request)
	case "/login":
		app.handleLogin(response, request)
	case "/logout":
		app.handleLogout(response, request)
	case "/upload":
		app.handleUpload(response, request)
	case "/assets/paper.css":
		if request.Method != http.MethodGet {
			http.Error(response, "Method not allowed", http.StatusMethodNotAllowed)
			return
		}
		response.Header().Set("Content-Type", "text/css; charset=utf-8")
		_, _ = response.Write([]byte(paperCSS))
	default:
		http.NotFound(response, request)
	}
}

func restartDashboard() error {
	context, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	output, err := exec.CommandContext(context, "/etc/init.d/nes-deck", "restart").CombinedOutput()
	if err != nil {
		return fmt.Errorf("restart failed: %w (%s)", err, strings.TrimSpace(string(output)))
	}
	return nil
}

func listenWireGuard() (net.Listener, error) {
	configuration := net.ListenConfig{
		Control: func(network, address string, connection syscall.RawConn) error {
			var controlError error
			if err := connection.Control(func(descriptor uintptr) {
				controlError = syscall.SetsockoptString(int(descriptor), syscall.SOL_SOCKET, 25, serviceInterface)
			}); err != nil {
				return err
			}
			return controlError
		},
	}
	return configuration.Listen(context.Background(), "tcp4", serviceAddress)
}
