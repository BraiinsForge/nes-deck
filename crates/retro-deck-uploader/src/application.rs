//! Authenticated uploader routes over the bounded HTTP primitives.

use std::{
    fmt, io,
    net::{IpAddr, SocketAddrV4},
    time::{Duration, Instant},
};

use crate::{
    auth::{AuthError, AuthManager, LoginOutcome, Session, SessionCookie},
    form::{MAXIMUM_UPLOAD_REQUEST_BYTES, RomUploadForm, UrlEncodedForm, UrlEncodedLimits},
    http::{
        BAD_REQUEST, FORBIDDEN, METHOD_NOT_ALLOWED, MISDIRECTED_REQUEST, Method, NOT_FOUND, OK,
        Request, RequestHead, Response, ResponseError, SEE_OTHER, Status, TOO_MANY_REQUESTS,
        UNAUTHORIZED, UNPROCESSABLE_CONTENT, UNSUPPORTED_MEDIA_TYPE,
    },
    palette::{Palette, PaletteField, PaletteStore},
    store::RomStore,
    web::{PALETTE_JS, PAPER_CSS, Page},
};

const SERVICE_PORT: u16 = 8_080;
const SESSION_COOKIE_NAME: &str = "deck_rom_session";
const LOGIN_FORM_LIMITS: UrlEncodedLimits = UrlEncodedLimits::new(512, 1, 16, 128);
const ACTION_FORM_LIMITS: UrlEncodedLimits = UrlEncodedLimits::new(512, 1, 16, 128);
const PALETTE_FORM_LIMITS: UrlEncodedLimits = UrlEncodedLimits::new(4_096, 23, 32, 128);

/// Process-control boundary kept separate from durable stores.
pub trait DashboardRestarter: Send + Sync {
    /// Restart the dashboard after a catalog or palette mutation.
    ///
    /// # Errors
    ///
    /// Returns an operating-system or process failure. A completed store
    /// mutation remains durable and the UI tells the user it is pending.
    fn restart(&self) -> io::Result<()>;
}

/// Shared, authenticated HTTP application state.
pub struct Application {
    auth: AuthManager,
    roms: RomStore,
    palette: PaletteStore,
    restarter: Box<dyn DashboardRestarter>,
}

impl Application {
    /// Assemble already-validated authentication, storage, and process edges.
    #[must_use]
    pub fn new(
        auth: AuthManager,
        roms: RomStore,
        palette: PaletteStore,
        restarter: Box<dyn DashboardRestarter>,
    ) -> Self {
        Self {
            auth,
            roms,
            palette,
            restarter,
        }
    }

    /// Return the body limit for a parsed request before its body is read.
    ///
    /// Unknown routes and wrong methods receive a zero-byte allowance.
    #[must_use]
    pub fn maximum_body_bytes(&self, head: &RequestHead) -> usize {
        if head.method() != Method::Post {
            return 0;
        }
        match head.path() {
            "/login" | "/logout" => 512,
            "/palette" => 4_096,
            "/upload" => MAXIMUM_UPLOAD_REQUEST_BYTES,
            _ => 0,
        }
    }

    /// Dispatch one complete request and add the fixed browser security policy.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError`] only for authentication-state corruption,
    /// entropy failure, or an internally invalid response header. Expected
    /// form, authorization, storage, and restart failures become responses.
    pub fn handle(
        &self,
        request: Request,
        source: IpAddr,
        now: Instant,
    ) -> Result<Response, ApplicationError> {
        if !valid_host(request.head()) {
            return Ok(Response::text(
                MISDIRECTED_REQUEST,
                "Use one of this Deck's IPv4 addresses on port 8080.",
            )
            .hardened());
        }
        self.dispatch(request, source, now).map(Response::hardened)
    }

    fn dispatch(
        &self,
        request: Request,
        source: IpAddr,
        now: Instant,
    ) -> Result<Response, ApplicationError> {
        let method = request.head().method();
        let path = request.head().path().to_owned();
        match path.as_str() {
            "/" => match method {
                Method::Get => self.index(&request, now),
                Method::Post | Method::Other => method_not_allowed("GET"),
            },
            "/login" => match method {
                Method::Post => self.login(&request, source, now),
                Method::Get | Method::Other => method_not_allowed("POST"),
            },
            "/logout" => match method {
                Method::Post => self.logout(&request, now),
                Method::Get | Method::Other => method_not_allowed("POST"),
            },
            "/upload" => match method {
                Method::Post => self.upload(request, now),
                Method::Get | Method::Other => method_not_allowed("POST"),
            },
            "/palette" => match method {
                Method::Post => self.save_palette(&request, now),
                Method::Get | Method::Other => method_not_allowed("POST"),
            },
            "/assets/paper.css" => {
                static_asset(method, "text/css; charset=utf-8", PAPER_CSS.as_bytes())
            }
            "/assets/palette.js" => static_asset(
                method,
                "text/javascript; charset=utf-8",
                PALETTE_JS.as_bytes(),
            ),
            _ => Ok(Response::text(NOT_FOUND, "Not found")),
        }
    }

    fn index(&self, request: &Request, now: Instant) -> Result<Response, ApplicationError> {
        match self.current_session(request.head(), now)? {
            Some(session) => Ok(self.dashboard(&session, OK, None, None)),
            None => Ok(Response::html(OK, Page::login(None).render())),
        }
    }

    fn login(
        &self,
        request: &Request,
        source: IpAddr,
        now: Instant,
    ) -> Result<Response, ApplicationError> {
        if !same_origin(request.head()) {
            return Ok(Response::text(FORBIDDEN, "Cross-origin request rejected"));
        }
        if !urlencoded_content_type(request.head()) {
            return Ok(Response::text(
                UNSUPPORTED_MEDIA_TYPE,
                "Unsupported form encoding",
            ));
        }
        let form = match UrlEncodedForm::parse(request.body(), LOGIN_FORM_LIMITS) {
            Ok(form) if form.len() == 1 && form.get("password").is_some() => form,
            Ok(_) | Err(_) => {
                return Ok(Response::html(
                    BAD_REQUEST,
                    Page::login(Some("The login form was malformed.")).render(),
                ));
            }
        };
        let password = form.get("password").unwrap_or_default();
        match self.auth.login(source, password, now)? {
            LoginOutcome::Accepted(cookie) => redirect_with_session(&cookie),
            LoginOutcome::Rejected => Ok(Response::html(
                UNAUTHORIZED,
                Page::login(Some("That password was not accepted.")).render(),
            )),
            LoginOutcome::Blocked(remaining) => {
                let mut response = Response::html(
                    TOO_MANY_REQUESTS,
                    Page::login(Some(
                        "Too many attempts. Wait five minutes before trying again.",
                    ))
                    .render(),
                );
                response.add_header("Retry-After", retry_after(remaining).to_string())?;
                Ok(response)
            }
            LoginOutcome::Busy => {
                let mut response = Response::html(
                    TOO_MANY_REQUESTS,
                    Page::login(Some(
                        "Another sign-in is being checked. Try again in a moment.",
                    ))
                    .render(),
                );
                response.add_header("Retry-After", "3")?;
                Ok(response)
            }
        }
    }

    fn logout(&self, request: &Request, now: Instant) -> Result<Response, ApplicationError> {
        if !same_origin(request.head()) {
            return Ok(Response::text(FORBIDDEN, "Request rejected"));
        }
        if !urlencoded_content_type(request.head()) {
            return Ok(Response::text(
                UNSUPPORTED_MEDIA_TYPE,
                "Unsupported form encoding",
            ));
        }
        let Some(session) = self.current_session(request.head(), now)? else {
            return Ok(Response::text(UNAUTHORIZED, "Authentication required"));
        };
        let form = UrlEncodedForm::parse(request.body(), ACTION_FORM_LIMITS);
        let csrf = form
            .as_ref()
            .ok()
            .and_then(|form| (form.len() == 1).then(|| form.get("csrf")).flatten());
        let Some(csrf) = csrf else {
            return Ok(Response::text(FORBIDDEN, "Request rejected"));
        };
        if !self.auth.logout(&session, csrf, now)? {
            return Ok(Response::text(FORBIDDEN, "Request rejected"));
        }
        redirect_without_session()
    }

    fn upload(&self, request: Request, now: Instant) -> Result<Response, ApplicationError> {
        if !same_origin(request.head()) {
            return Ok(Response::text(FORBIDDEN, "Request rejected"));
        }
        let Some(session) = self.current_session(request.head(), now)? else {
            return Ok(Response::text(UNAUTHORIZED, "Authentication required"));
        };
        let Some(content_type) = request
            .head()
            .text_header("content-type")
            .map(str::to_owned)
        else {
            return Ok(Response::text(
                UNSUPPORTED_MEDIA_TYPE,
                "Unsupported form encoding",
            ));
        };
        let Ok(form) = RomUploadForm::parse(&content_type, request.into_body()) else {
            return Ok(self.dashboard(
                &session,
                BAD_REQUEST,
                Some("The upload form was malformed or too large."),
                None,
            ));
        };
        if !session.csrf_matches(form.csrf()) {
            return Ok(Response::text(FORBIDDEN, "Request rejected"));
        }
        let entry = match self.roms.add(
            form.system(),
            form.title(),
            form.filename(),
            form.contents(),
        ) {
            Ok(entry) => entry,
            Err(error) => {
                let message = error.to_string();
                return Ok(self.dashboard(&session, UNPROCESSABLE_CONTENT, Some(&message), None));
            }
        };
        let notice = if self.restarter.restart().is_ok() {
            format!(
                "{} was validated, filed, and added to the dashboard.",
                entry.title()
            )
        } else {
            format!(
                "{} was saved. The dashboard will pick it up after its next restart.",
                entry.title()
            )
        };
        Ok(self.dashboard(&session, OK, None, Some(&notice)))
    }

    fn save_palette(&self, request: &Request, now: Instant) -> Result<Response, ApplicationError> {
        if !same_origin(request.head()) {
            return Ok(Response::text(FORBIDDEN, "Request rejected"));
        }
        if !urlencoded_content_type(request.head()) {
            return Ok(Response::text(
                UNSUPPORTED_MEDIA_TYPE,
                "Unsupported form encoding",
            ));
        }
        let Some(session) = self.current_session(request.head(), now)? else {
            return Ok(Response::text(UNAUTHORIZED, "Authentication required"));
        };
        let Ok(form) = UrlEncodedForm::parse(request.body(), PALETTE_FORM_LIMITS) else {
            return Ok(self.dashboard(
                &session,
                BAD_REQUEST,
                Some("The appearance form was malformed."),
                None,
            ));
        };
        let Some(csrf) = form.get("csrf") else {
            return Ok(Response::text(FORBIDDEN, "Request rejected"));
        };
        if !session.csrf_matches(csrf) {
            return Ok(Response::text(FORBIDDEN, "Request rejected"));
        }
        let palette = match Palette::from_pairs(form.iter().filter(|(name, _)| *name != "csrf")) {
            Ok(palette) => palette,
            Err(error) => {
                let message = error.to_string();
                return Ok(self.dashboard(&session, BAD_REQUEST, Some(&message), None));
            }
        };
        if let Err(error) = self.palette.save(&palette) {
            let message = error.to_string();
            return Ok(self.dashboard(&session, UNPROCESSABLE_CONTENT, Some(&message), None));
        }
        let notice = if self.restarter.restart().is_ok() {
            "Dashboard appearance was saved and applied."
        } else {
            "Dashboard appearance was saved and will apply after the next restart."
        };
        Ok(self.dashboard(&session, OK, None, Some(notice)))
    }

    fn current_session(
        &self,
        head: &RequestHead,
        now: Instant,
    ) -> Result<Option<Session>, AuthError> {
        let Some(token) = session_token(head) else {
            return Ok(None);
        };
        self.auth.session(token, now)
    }

    fn dashboard(
        &self,
        session: &Session,
        status: Status,
        error: Option<&str>,
        notice: Option<&str>,
    ) -> Response {
        let mut message = error.map(str::to_owned);
        let palette = if let Ok(palette) = self.palette.current() {
            palette
        } else {
            if message.is_none() {
                message = Some("The dashboard appearance cannot be read.".to_owned());
            }
            Vec::<PaletteField>::new()
        };
        Response::html(
            status,
            Page::dashboard(session.csrf(), &palette, message.as_deref(), notice).render(),
        )
    }
}

impl fmt::Debug for Application {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Application")
            .field("auth", &self.auth)
            .field("roms", &self.roms)
            .field("palette", &self.palette)
            .finish_non_exhaustive()
    }
}

fn valid_host(head: &RequestHead) -> bool {
    head.text_header("host")
        .and_then(|host| host.parse::<SocketAddrV4>().ok())
        .is_some_and(|address| address.port() == SERVICE_PORT)
}

fn same_origin(head: &RequestHead) -> bool {
    if let Some(site) = head.header("sec-fetch-site") {
        let Ok(site) = std::str::from_utf8(site) else {
            return false;
        };
        if !["same-origin", "same-site", "none"]
            .iter()
            .any(|allowed| site.eq_ignore_ascii_case(allowed))
        {
            return false;
        }
    }
    let Some(host) = head.text_header("host") else {
        return false;
    };
    match head.header("origin") {
        None => true,
        Some(origin) => {
            let Ok(origin) = std::str::from_utf8(origin) else {
                return false;
            };
            origin == "null" || origin == format!("http://{host}")
        }
    }
}

fn urlencoded_content_type(head: &RequestHead) -> bool {
    let Some(content_type) = head.text_header("content-type") else {
        return false;
    };
    let mut parts = content_type.split(';').map(str::trim);
    if !parts.next().is_some_and(|media_type| {
        media_type.eq_ignore_ascii_case("application/x-www-form-urlencoded")
    }) {
        return false;
    }
    let mut saw_charset = false;
    for parameter in parts {
        let Some((name, value)) = parameter.split_once('=') else {
            return false;
        };
        if saw_charset || !name.trim().eq_ignore_ascii_case("charset") {
            return false;
        }
        let value = value.trim();
        let value = if let Some(quoted) = value.strip_prefix('"') {
            let Some(quoted) = quoted.strip_suffix('"') else {
                return false;
            };
            quoted
        } else if value.contains('"') {
            return false;
        } else {
            value
        };
        if !value.eq_ignore_ascii_case("utf-8") {
            return false;
        }
        saw_charset = true;
    }
    true
}

fn session_token(head: &RequestHead) -> Option<&str> {
    let cookie = head.text_header("cookie")?;
    let mut found = None;
    for pair in cookie.split(';') {
        let (name, value) = pair.trim().split_once('=')?;
        if name.trim() == SESSION_COOKIE_NAME {
            if found.is_some() {
                return None;
            }
            found = Some(value.trim());
        }
    }
    found
}

fn redirect_with_session(cookie: &SessionCookie) -> Result<Response, ApplicationError> {
    let mut response = Response::text(SEE_OTHER, "");
    response.add_header("Location", "/")?;
    response.add_header(
        "Set-Cookie",
        format!(
            "{SESSION_COOKIE_NAME}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Strict",
            cookie.token(),
            cookie.max_age_seconds()
        ),
    )?;
    Ok(response)
}

fn redirect_without_session() -> Result<Response, ApplicationError> {
    let mut response = Response::text(SEE_OTHER, "");
    response.add_header("Location", "/")?;
    response.add_header(
        "Set-Cookie",
        format!("{SESSION_COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict"),
    )?;
    Ok(response)
}

fn method_not_allowed(allow: &'static str) -> Result<Response, ApplicationError> {
    let mut response = Response::text(METHOD_NOT_ALLOWED, "Method not allowed");
    response.add_header("Allow", allow)?;
    Ok(response)
}

fn static_asset(
    method: Method,
    content_type: &'static str,
    body: &'static [u8],
) -> Result<Response, ApplicationError> {
    match method {
        Method::Get => Response::asset(OK, content_type, body).map_err(Into::into),
        Method::Post | Method::Other => method_not_allowed("GET"),
    }
}

fn retry_after(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() != 0))
        .max(1)
}

/// Authentication or response-construction failure during dispatch.
#[derive(Debug)]
pub enum ApplicationError {
    /// Authentication entropy, time, or synchronization failed.
    Auth(AuthError),
    /// An internally constructed response header violated its invariant.
    Response(ResponseError),
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth(error) => write!(formatter, "authentication failed: {error}"),
            Self::Response(error) => write!(formatter, "response construction failed: {error}"),
        }
    }
}

impl std::error::Error for ApplicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Auth(error) => Some(error),
            Self::Response(error) => Some(error),
        }
    }
}

impl From<AuthError> for ApplicationError {
    fn from(error: AuthError) -> Self {
        Self::Auth(error)
    }
}

impl From<ResponseError> for ApplicationError {
    fn from(error: ResponseError) -> Self {
        Self::Response(error)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{self, BufReader, Cursor},
        net::IpAddr,
        str::FromStr as _,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Instant,
    };

    use crate::{
        auth::LoginOutcome,
        catalog::load,
        http::{
            FORBIDDEN, METHOD_NOT_ALLOWED, MISDIRECTED_REQUEST, OK, SEE_OTHER, UNAUTHORIZED,
            UNPROCESSABLE_CONTENT, read_request_body, read_request_head,
        },
        password::PasswordConfig,
    };

    use super::*;

    const HOST: &str = "192.0.2.10:8080";
    const SOURCE: &str = "192.0.2.40";
    const PASSWORD: &str = "configured-password";

    struct FakeRestarter {
        calls: Arc<AtomicUsize>,
        fail: bool,
    }

    impl DashboardRestarter for FakeRestarter {
        fn restart(&self) -> io::Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(io::Error::other("test restart failure"))
            } else {
                Ok(())
            }
        }
    }

    struct Fixture {
        application: Application,
        directory: tempfile::TempDir,
        restarts: Arc<AtomicUsize>,
    }

    impl Fixture {
        fn new(restart_fails: bool) -> Option<Self> {
            let directory = tempfile::tempdir().ok()?;
            let base_catalog = directory.path().join("base.tsv");
            let upload_catalog = directory.path().join("uploads.tsv");
            let fallback_palette = directory.path().join("fallback.tsv");
            let active_palette = directory.path().join("active.tsv");
            let override_palette = directory.path().join("override.sexp");
            fs::write(&base_catalog, b"").ok()?;
            fs::write(
                &fallback_palette,
                include_bytes!("../../../deploy/menu/palette.tsv"),
            )
            .ok()?;
            fs::write(
                &active_palette,
                include_bytes!("../../../deploy/menu/palette.tsv"),
            )
            .ok()?;
            let roms =
                RomStore::new(directory.path().join("roms"), base_catalog, upload_catalog).ok()?;
            let palette =
                PaletteStore::new(active_palette, fallback_palette, override_palette).ok()?;
            let password = PasswordConfig::new(PASSWORD).ok()?;
            let restarts = Arc::new(AtomicUsize::new(0));
            let restarter = FakeRestarter {
                calls: Arc::clone(&restarts),
                fail: restart_fails,
            };
            Some(Self {
                application: Application::new(
                    AuthManager::new(password),
                    roms,
                    palette,
                    Box::new(restarter),
                ),
                directory,
                restarts,
            })
        }

        fn session(&self, now: Instant) -> Option<(String, String)> {
            let source = IpAddr::from_str(SOURCE).ok()?;
            let LoginOutcome::Accepted(cookie) =
                self.application.auth.login(source, PASSWORD, now).ok()?
            else {
                return None;
            };
            let session = self.application.auth.session(cookie.token(), now).ok()??;
            Some((cookie.token().to_owned(), session.csrf().to_owned()))
        }

        fn restart_count(&self) -> usize {
            self.restarts.load(Ordering::SeqCst)
        }
    }

    fn source() -> Option<IpAddr> {
        IpAddr::from_str(SOURCE).ok()
    }

    fn request(
        method: &str,
        path: &str,
        host: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Option<Request> {
        let mut bytes = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\n",
            body.len()
        )
        .into_bytes();
        for (name, value) in headers {
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(b": ");
            bytes.extend_from_slice(value.as_bytes());
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(b"\r\n");
        bytes.extend_from_slice(body);
        let mut reader = BufReader::new(Cursor::new(bytes));
        let head = read_request_head(&mut reader).ok()?;
        read_request_body(&mut reader, head, MAXIMUM_UPLOAD_REQUEST_BYTES).ok()
    }

    fn handle(fixture: &Fixture, request: Request, now: Instant) -> Option<Response> {
        fixture.application.handle(request, source()?, now).ok()
    }

    fn text(response: &Response) -> &str {
        std::str::from_utf8(response.body()).unwrap_or_default()
    }

    fn cookie_header(token: &str) -> String {
        format!("{SESSION_COOKIE_NAME}={token}")
    }

    fn origin_headers<'a>(content_type: &'a str, cookie: &'a str) -> [(&'a str, &'a str); 3] {
        [
            ("Content-Type", content_type),
            ("Cookie", cookie),
            ("Origin", "http://192.0.2.10:8080"),
        ]
    }

    fn multipart(csrf: &str, title: &str, rom: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, value) in [
            ("csrf", csrf.as_bytes()),
            ("system", b"chip8"),
            ("title", title.as_bytes()),
        ] {
            body.extend_from_slice(b"--test-boundary\r\nContent-Disposition: form-data; name=\"");
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(b"\"\r\n\r\n");
            body.extend_from_slice(value);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(
            b"--test-boundary\r\nContent-Disposition: form-data; name=\"rom\"; filename=\"game.ch8\"\r\nContent-Type: application/octet-stream\r\n\r\n",
        );
        body.extend_from_slice(rom);
        body.extend_from_slice(b"\r\n--test-boundary--\r\n");
        body
    }

    #[test]
    fn public_routes_validate_host_method_limits_and_headers() {
        let Some(fixture) = Fixture::new(false) else {
            return;
        };
        let now = Instant::now();
        let Some(index) =
            request("GET", "/", HOST, &[], b"").and_then(|request| handle(&fixture, request, now))
        else {
            return;
        };
        assert_eq!(index.status(), OK);
        assert!(text(&index).contains("name=\"password\""));
        assert_eq!(index.header("Cache-Control"), Some("no-store"));
        assert_eq!(index.header("X-Frame-Options"), Some("DENY"));
        assert!(index.header("Content-Security-Policy").is_some());

        let Some(asset) = request("GET", "/assets/paper.css", HOST, &[], b"")
            .and_then(|request| handle(&fixture, request, now))
        else {
            return;
        };
        assert_eq!(asset.status(), OK);
        assert_eq!(
            asset.header("Content-Type"),
            Some("text/css; charset=utf-8")
        );

        let Some(wrong_method) = request("GET", "/login", HOST, &[], b"")
            .and_then(|request| handle(&fixture, request, now))
        else {
            return;
        };
        assert_eq!(wrong_method.status(), METHOD_NOT_ALLOWED);
        assert_eq!(wrong_method.header("Allow"), Some("POST"));

        let Some(wrong_host) = request("GET", "/", "retrodeck.local:8080", &[], b"")
            .and_then(|request| handle(&fixture, request, now))
        else {
            return;
        };
        assert_eq!(wrong_host.status(), MISDIRECTED_REQUEST);

        let raw = b"POST /upload HTTP/1.1\r\nHost: 192.0.2.10:8080\r\nContent-Length: 0\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw));
        let head = read_request_head(&mut reader);
        assert!(head.is_ok());
        let Some(head) = head.ok() else {
            return;
        };
        assert_eq!(
            fixture.application.maximum_body_bytes(&head),
            MAXIMUM_UPLOAD_REQUEST_BYTES
        );
    }

    #[test]
    fn login_dashboard_and_logout_round_trip() {
        let Some(fixture) = Fixture::new(false) else {
            return;
        };
        let Some(source) = source() else {
            return;
        };
        let now = Instant::now();
        let body = format!("password={PASSWORD}");
        let cross_origin = [
            ("Content-Type", "application/x-www-form-urlencoded"),
            ("Origin", "http://198.51.100.7:8080"),
        ];
        let Some(rejected) = request("POST", "/login", HOST, &cross_origin, body.as_bytes())
            .and_then(|request| fixture.application.handle(request, source, now).ok())
        else {
            return;
        };
        assert_eq!(rejected.status(), FORBIDDEN);

        let headers = [
            (
                "Content-Type",
                "application/x-www-form-urlencoded; charset=UTF-8",
            ),
            ("Origin", "http://192.0.2.10:8080"),
        ];
        let Some(login) = request("POST", "/login", HOST, &headers, body.as_bytes())
            .and_then(|request| fixture.application.handle(request, source, now).ok())
        else {
            return;
        };
        assert_eq!(login.status(), SEE_OTHER);
        assert_eq!(login.header("Location"), Some("/"));
        let Some(set_cookie) = login.header("Set-Cookie") else {
            return;
        };
        let Some(token) = set_cookie
            .split(';')
            .next()
            .and_then(|pair| pair.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")))
        else {
            return;
        };
        let cookie = cookie_header(token);
        let Some(dashboard) = request("GET", "/", HOST, &[("Cookie", &cookie)], b"")
            .and_then(|request| fixture.application.handle(request, source, now).ok())
        else {
            return;
        };
        assert_eq!(dashboard.status(), OK);
        assert!(text(&dashboard).contains("action=\"/upload\""));
        let session = fixture.application.auth.session(token, now);
        let Ok(Some(session)) = session else {
            return;
        };
        let logout_body = format!("csrf={}", session.csrf());
        let logout_headers = origin_headers("application/x-www-form-urlencoded", &cookie);
        let Some(logout) = request(
            "POST",
            "/logout",
            HOST,
            &logout_headers,
            logout_body.as_bytes(),
        )
        .and_then(|request| fixture.application.handle(request, source, now).ok()) else {
            return;
        };
        assert_eq!(logout.status(), SEE_OTHER);
        assert!(
            logout
                .header("Set-Cookie")
                .is_some_and(|value| value.contains("Max-Age=0"))
        );

        let Some(after) = request("GET", "/", HOST, &[("Cookie", &cookie)], b"")
            .and_then(|request| fixture.application.handle(request, source, now).ok())
        else {
            return;
        };
        assert_eq!(after.status(), OK);
        assert!(text(&after).contains("name=\"password\""));
        assert!(!text(&after).contains("action=\"/upload\""));
    }

    #[test]
    fn upload_is_csrf_checked_filed_once_and_restarted() {
        let Some(fixture) = Fixture::new(false) else {
            return;
        };
        let now = Instant::now();
        let Some((token, csrf)) = fixture.session(now) else {
            return;
        };
        let cookie = cookie_header(&token);
        let headers = origin_headers("multipart/form-data; boundary=test-boundary", &cookie);
        let body = multipart(&csrf, "Space Racer", b"\x00\xe0");
        let Some(uploaded) = request("POST", "/upload", HOST, &headers, &body)
            .and_then(|request| handle(&fixture, request, now))
        else {
            return;
        };
        assert_eq!(uploaded.status(), OK);
        assert!(text(&uploaded).contains("validated, filed, and added"));
        assert_eq!(fixture.restart_count(), 1);
        assert_eq!(
            fs::read(fixture.directory.path().join("roms/chip8/space-racer.ch8")).ok(),
            Some(b"\x00\xe0".to_vec())
        );
        let catalog = load(&fixture.directory.path().join("uploads.tsv"));
        assert!(matches!(catalog, Ok(catalog) if catalog.len() == 1));

        let duplicate = request("POST", "/upload", HOST, &headers, &body)
            .and_then(|request| handle(&fixture, request, now));
        assert!(matches!(duplicate, Some(response) if response.status() == UNPROCESSABLE_CONTENT));
        assert_eq!(fixture.restart_count(), 1);

        let bad_csrf = multipart(
            "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
            "Another Game",
            b"\x01",
        );
        let rejected = request("POST", "/upload", HOST, &headers, &bad_csrf)
            .and_then(|request| handle(&fixture, request, now));
        assert!(matches!(rejected, Some(response) if response.status() == FORBIDDEN));
        assert!(
            !fixture
                .directory
                .path()
                .join("roms/chip8/another-game.ch8")
                .exists()
        );
    }

    #[test]
    fn palette_save_survives_restart_failure() {
        let Some(fixture) = Fixture::new(true) else {
            return;
        };
        let now = Instant::now();
        let Some((token, csrf)) = fixture.session(now) else {
            return;
        };
        let cookie = cookie_header(&token);
        let Ok(fields) = fixture.application.palette.current() else {
            return;
        };
        let mut body = format!("csrf={csrf}");
        for field in fields {
            body.push('&');
            body.push_str(field.name);
            body.push('=');
            if field.name == "background" {
                body.push_str("%23123456");
            } else {
                body.push_str("%23");
                body.push_str(field.value.trim_start_matches('#'));
            }
        }
        let headers = origin_headers("application/x-www-form-urlencoded", &cookie);
        let Some(response) = request("POST", "/palette", HOST, &headers, body.as_bytes())
            .and_then(|request| handle(&fixture, request, now))
        else {
            return;
        };
        assert_eq!(response.status(), OK);
        assert!(text(&response).contains("will apply after the next restart"));
        assert_eq!(fixture.restart_count(), 1);
        let current = fixture.application.palette.current();
        assert!(matches!(
            current,
            Ok(fields) if fields.iter().any(|field| field.name == "background" && field.value == "#123456")
        ));
        assert!(fixture.directory.path().join("override.sexp").exists());
    }

    #[test]
    fn unauthenticated_mutations_are_rejected() {
        let Some(fixture) = Fixture::new(false) else {
            return;
        };
        let now = Instant::now();
        let headers = [
            ("Content-Type", "application/x-www-form-urlencoded"),
            ("Origin", "http://192.0.2.10:8080"),
        ];
        let palette = request("POST", "/palette", HOST, &headers, b"csrf=none")
            .and_then(|request| handle(&fixture, request, now));
        assert!(matches!(palette, Some(response) if response.status() == UNAUTHORIZED));
    }
}
