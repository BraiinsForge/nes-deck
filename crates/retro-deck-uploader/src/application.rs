//! Authenticated uploader routes built on Axum's bounded HTTP primitives.

use std::{
    collections::HashSet,
    fmt, io,
    net::{SocketAddr, SocketAddrV4},
    str::{self, FromStr as _},
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Form, Multipart, Request, State},
    http::{
        HeaderMap, HeaderName, HeaderValue, StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST, ORIGIN, REFERRER_POLICY,
            RETRY_AFTER, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
        },
    },
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};

use crate::{
    auth::{AuthError, AuthManager, LoginOutcome, Session, SessionCookie},
    palette::{Palette, PaletteField, PaletteStore},
    rom::{GameTitle, System},
    store::RomStore,
    web::{PALETTE_JS, PAPER_CSS, Page},
};

const SERVICE_PORT: u16 = 8_080;
const SESSION_COOKIE_NAME: &str = "deck_rom_session";
const LOGIN_FORM_BYTES: usize = 512;
const ACTION_FORM_BYTES: usize = 512;
const PALETTE_FORM_BYTES: usize = 4_096;
const MAXIMUM_UPLOAD_REQUEST_BYTES: usize = 12 * 1_024 * 1_024;
const MAXIMUM_UPLOAD_PARTS: usize = 4;
const MAXIMUM_FILENAME_BYTES: usize = 255;
const MAXIMUM_TEXT_FIELD_BYTES: usize = 256;
const CROSS_ORIGIN_OPENER_POLICY: HeaderName =
    HeaderName::from_static("cross-origin-opener-policy");
const CROSS_ORIGIN_RESOURCE_POLICY: HeaderName =
    HeaderName::from_static("cross-origin-resource-policy");
const PERMISSIONS_POLICY: HeaderName = HeaderName::from_static("permissions-policy");

type FormFields = Vec<(String, String)>;

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

/// Shared, authenticated uploader state.
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

    /// Build the complete HTTP router around this application's shared state.
    pub fn router(self: Arc<Self>) -> Router {
        let mutations = Router::new()
            .route(
                "/login",
                post(login).layer(DefaultBodyLimit::max(LOGIN_FORM_BYTES)),
            )
            .route(
                "/logout",
                post(logout).layer(DefaultBodyLimit::max(ACTION_FORM_BYTES)),
            )
            .route(
                "/upload",
                post(upload).layer(DefaultBodyLimit::max(MAXIMUM_UPLOAD_REQUEST_BYTES)),
            )
            .route(
                "/palette",
                post(save_palette).layer(DefaultBodyLimit::max(PALETTE_FORM_BYTES)),
            )
            .route_layer(middleware::from_fn(require_same_origin));

        Router::new()
            .route("/", get(index))
            .route("/assets/paper.css", get(paper_css))
            .route("/assets/palette.js", get(palette_js))
            .merge(mutations)
            .fallback(not_found)
            .layer(middleware::from_fn(secure_request))
            .with_state(self)
    }

    fn current_session(
        &self,
        cookies: &CookieJar,
        now: Instant,
    ) -> Result<Option<Session>, AuthError> {
        let Some(token) = cookies.get(SESSION_COOKIE_NAME).map(Cookie::value) else {
            return Ok(None);
        };
        self.auth.session(token, now)
    }

    fn dashboard(
        &self,
        session: &Session,
        status: StatusCode,
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
        (
            status,
            Html(Page::dashboard(session.csrf(), &palette, message.as_deref(), notice).render()),
        )
            .into_response()
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

async fn index(State(application): State<Arc<Application>>, cookies: CookieJar) -> Response {
    match application.current_session(&cookies, Instant::now()) {
        Ok(Some(session)) => application.dashboard(&session, StatusCode::OK, None, None),
        Ok(None) => Html(Page::login(None).render()).into_response(),
        Err(error) => internal_auth_error(&error),
    }
}

async fn login(
    State(application): State<Arc<Application>>,
    ConnectInfo(source): ConnectInfo<SocketAddr>,
    cookies: CookieJar,
    Form(fields): Form<FormFields>,
) -> Response {
    let [(name, password)] = fields.as_slice() else {
        return malformed_login();
    };
    if name != "password" || password.len() > 128 {
        return malformed_login();
    }
    match application
        .auth
        .login(source.ip(), password, Instant::now())
    {
        Ok(LoginOutcome::Accepted(cookie)) => redirect_with_session(cookies, &cookie),
        Ok(LoginOutcome::Rejected) => (
            StatusCode::UNAUTHORIZED,
            Html(Page::login(Some("That password was not accepted.")).render()),
        )
            .into_response(),
        Ok(LoginOutcome::Blocked(remaining)) => login_throttled(
            "Too many attempts. Wait five minutes before trying again.",
            retry_after(remaining),
        ),
        Ok(LoginOutcome::Busy) => login_throttled(
            "Another sign-in is being checked. Try again in a moment.",
            3,
        ),
        Err(error) => internal_auth_error(&error),
    }
}

async fn logout(
    State(application): State<Arc<Application>>,
    cookies: CookieJar,
    Form(fields): Form<FormFields>,
) -> Response {
    let now = Instant::now();
    let session = match application.current_session(&cookies, now) {
        Ok(Some(session)) => session,
        Ok(None) => return unauthorized(),
        Err(error) => return internal_auth_error(&error),
    };
    let [(name, csrf)] = fields.as_slice() else {
        return rejected();
    };
    if name != "csrf" {
        return rejected();
    }
    match application.auth.logout(&session, csrf, now) {
        Ok(true) => {
            let cookie = Cookie::build(SESSION_COOKIE_NAME).path("/").build();
            (cookies.remove(cookie), Redirect::to("/")).into_response()
        }
        Ok(false) => rejected(),
        Err(error) => internal_auth_error(&error),
    }
}

async fn upload(
    State(application): State<Arc<Application>>,
    cookies: CookieJar,
    multipart: Multipart,
) -> Response {
    let session = match application.current_session(&cookies, Instant::now()) {
        Ok(Some(session)) => session,
        Ok(None) => return unauthorized(),
        Err(error) => return internal_auth_error(&error),
    };
    let form = match UploadForm::read(multipart).await {
        Ok(form) => form,
        Err(error) => {
            eprintln!("rom-uploader: rejected multipart form: {error:?}");
            return application.dashboard(
                &session,
                StatusCode::BAD_REQUEST,
                Some("The upload form was malformed or too large."),
                None,
            );
        }
    };
    if !session.csrf_matches(&form.csrf) {
        return rejected();
    }
    let entry = match application.roms.add(
        form.system,
        &form.title,
        &form.filename,
        form.contents.as_ref(),
    ) {
        Ok(entry) => entry,
        Err(error) => {
            let message = error.to_string();
            return application.dashboard(
                &session,
                StatusCode::UNPROCESSABLE_ENTITY,
                Some(&message),
                None,
            );
        }
    };
    let notice = if application.restarter.restart().is_ok() {
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
    application.dashboard(&session, StatusCode::OK, None, Some(&notice))
}

async fn save_palette(
    State(application): State<Arc<Application>>,
    cookies: CookieJar,
    Form(fields): Form<FormFields>,
) -> Response {
    let session = match application.current_session(&cookies, Instant::now()) {
        Ok(Some(session)) => session,
        Ok(None) => return unauthorized(),
        Err(error) => return internal_auth_error(&error),
    };
    if !valid_unique_fields(&fields, 23, 32, 128) {
        return application.dashboard(
            &session,
            StatusCode::BAD_REQUEST,
            Some("The appearance form was malformed."),
            None,
        );
    }
    let Some(csrf) = fields
        .iter()
        .find_map(|(name, value)| (name == "csrf").then_some(value.as_str()))
    else {
        return rejected();
    };
    if !session.csrf_matches(csrf) {
        return rejected();
    }
    let palette = match Palette::from_pairs(
        fields
            .iter()
            .filter(|(name, _)| name != "csrf")
            .map(|(name, value)| (name.as_str(), value.as_str())),
    ) {
        Ok(palette) => palette,
        Err(error) => {
            let message = error.to_string();
            return application.dashboard(&session, StatusCode::BAD_REQUEST, Some(&message), None);
        }
    };
    if let Err(error) = application.palette.save(&palette) {
        let message = error.to_string();
        return application.dashboard(
            &session,
            StatusCode::UNPROCESSABLE_ENTITY,
            Some(&message),
            None,
        );
    }
    let notice = if application.restarter.restart().is_ok() {
        "Dashboard appearance was saved and applied."
    } else {
        "Dashboard appearance was saved and will apply after the next restart."
    };
    application.dashboard(&session, StatusCode::OK, None, Some(notice))
}

async fn paper_css() -> impl IntoResponse {
    ([(CONTENT_TYPE, "text/css; charset=utf-8")], PAPER_CSS)
}

async fn palette_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/javascript; charset=utf-8")],
        PALETTE_JS,
    )
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "Not found")
}

async fn secure_request(request: Request, next: Next) -> Response {
    let mut response = if valid_host(request.headers()) {
        next.run(request).await
    } else {
        (
            StatusCode::MISDIRECTED_REQUEST,
            "Use one of this Deck's IPv4 addresses on port 8080.",
        )
            .into_response()
    };
    harden(&mut response);
    response
}

async fn require_same_origin(request: Request, next: Next) -> Response {
    if same_origin(request.headers()) {
        next.run(request).await
    } else {
        (StatusCode::FORBIDDEN, "Cross-origin request rejected").into_response()
    }
}

fn harden(response: &mut Response) {
    for (name, value) in [
        (CACHE_CONTROL, "no-store"),
        (
            CONTENT_SECURITY_POLICY,
            "default-src 'none'; img-src 'self'; style-src 'self'; script-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
        (CROSS_ORIGIN_OPENER_POLICY, "same-origin"),
        (CROSS_ORIGIN_RESOURCE_POLICY, "same-origin"),
        (
            PERMISSIONS_POLICY,
            "camera=(), geolocation=(), microphone=()",
        ),
        (REFERRER_POLICY, "no-referrer"),
        (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (X_FRAME_OPTIONS, "DENY"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
}

fn valid_host(headers: &HeaderMap) -> bool {
    headers
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .and_then(|host| host.parse::<SocketAddrV4>().ok())
        .is_some_and(|address| address.port() == SERVICE_PORT)
}

fn same_origin(headers: &HeaderMap) -> bool {
    if let Some(site) = headers.get("sec-fetch-site") {
        let Ok(site) = site.to_str() else {
            return false;
        };
        if !["same-origin", "same-site", "none"]
            .iter()
            .any(|allowed| site.eq_ignore_ascii_case(allowed))
        {
            return false;
        }
    }
    let Some(host) = headers.get(HOST).and_then(|host| host.to_str().ok()) else {
        return false;
    };
    match headers.get(ORIGIN) {
        None => true,
        Some(origin) => origin
            .to_str()
            .is_ok_and(|origin| origin == "null" || origin == format!("http://{host}")),
    }
}

fn valid_unique_fields(
    fields: &FormFields,
    maximum_fields: usize,
    maximum_name_bytes: usize,
    maximum_value_bytes: usize,
) -> bool {
    if fields.len() > maximum_fields {
        return false;
    }
    let mut names = HashSet::with_capacity(fields.len());
    fields.iter().all(|(name, value)| {
        !name.is_empty()
            && name.len() <= maximum_name_bytes
            && value.len() <= maximum_value_bytes
            && name
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_lowercase())
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && names.insert(name.as_str())
    })
}

fn redirect_with_session(cookies: CookieJar, session: &SessionCookie) -> Response {
    let cookie = Cookie::build((SESSION_COOKIE_NAME, session.token().to_owned()))
        .path("/")
        .max_age(time::Duration::seconds(i64::from(
            session.max_age_seconds(),
        )))
        .http_only(true)
        .same_site(SameSite::Strict)
        .build();
    (cookies.add(cookie), Redirect::to("/")).into_response()
}

fn malformed_login() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Html(Page::login(Some("The login form was malformed.")).render()),
    )
        .into_response()
}

fn login_throttled(message: &str, seconds: u64) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Html(Page::login(Some(message)).render()),
    )
        .into_response();
    let value = HeaderValue::from_str(&seconds.to_string())
        .unwrap_or_else(|_| HeaderValue::from_static("1"));
    response.headers_mut().insert(RETRY_AFTER, value);
    response
}

fn retry_after(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() != 0))
        .max(1)
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "Authentication required").into_response()
}

fn rejected() -> Response {
    (StatusCode::FORBIDDEN, "Request rejected").into_response()
}

fn internal_auth_error(error: &AuthError) -> Response {
    eprintln!("rom-uploader: authentication state failed: {error}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
}

struct UploadForm {
    csrf: String,
    system: System,
    title: String,
    filename: String,
    contents: Bytes,
}

impl fmt::Debug for UploadForm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadForm")
            .field("csrf", &"[redacted]")
            .field("system", &self.system)
            .field("title", &self.title)
            .field("filename", &self.filename)
            .field("rom_bytes", &self.contents.len())
            .finish()
    }
}

impl UploadForm {
    async fn read(mut multipart: Multipart) -> Result<Self, UploadFormError> {
        let mut builder = UploadBuilder::default();
        let mut parts = 0_usize;
        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|_| UploadFormError::Malformed)?
        {
            parts = parts.saturating_add(1);
            if parts > MAXIMUM_UPLOAD_PARTS {
                return Err(UploadFormError::Malformed);
            }
            let name = field
                .name()
                .map(str::to_owned)
                .ok_or(UploadFormError::Malformed)?;
            let filename = field.file_name().map(str::to_owned);
            let has_content_type = field.content_type().is_some();
            let contents = field
                .bytes()
                .await
                .map_err(|_| UploadFormError::Malformed)?;
            builder.insert(&name, filename, has_content_type, contents)?;
        }
        builder.finish()
    }
}

#[derive(Debug, Default)]
struct UploadBuilder {
    csrf: Option<String>,
    system: Option<System>,
    title: Option<String>,
    filename: Option<String>,
    contents: Option<Bytes>,
}

impl UploadBuilder {
    fn insert(
        &mut self,
        name: &str,
        filename: Option<String>,
        has_content_type: bool,
        contents: Bytes,
    ) -> Result<(), UploadFormError> {
        match name {
            "csrf" if filename.is_none() && !has_content_type => {
                insert_once(&mut self.csrf, parse_csrf(contents.as_ref())?)
            }
            "system" if filename.is_none() && !has_content_type => {
                let value = parse_text(contents.as_ref())?;
                let system = System::from_str(value).map_err(|_| UploadFormError::InvalidField)?;
                insert_once(&mut self.system, system)
            }
            "title" if filename.is_none() && !has_content_type => {
                let value = parse_text(contents.as_ref())?;
                GameTitle::new(value).map_err(|_| UploadFormError::InvalidField)?;
                insert_once(&mut self.title, value.to_owned())
            }
            "rom" if filename.is_some() => {
                let filename = filename.ok_or(UploadFormError::InvalidFilename)?;
                validate_filename(&filename)?;
                if self.filename.is_some() || self.contents.is_some() {
                    return Err(UploadFormError::RepeatedField);
                }
                self.filename = Some(filename);
                self.contents = Some(contents);
                Ok(())
            }
            "csrf" | "system" | "title" | "rom" => Err(UploadFormError::Malformed),
            _ => Err(UploadFormError::UnexpectedField),
        }
    }

    fn finish(self) -> Result<UploadForm, UploadFormError> {
        Ok(UploadForm {
            csrf: self.csrf.ok_or(UploadFormError::MissingField)?,
            system: self.system.ok_or(UploadFormError::MissingField)?,
            title: self.title.ok_or(UploadFormError::MissingField)?,
            filename: self.filename.ok_or(UploadFormError::MissingField)?,
            contents: self.contents.ok_or(UploadFormError::MissingField)?,
        })
    }
}

fn insert_once<T>(destination: &mut Option<T>, value: T) -> Result<(), UploadFormError> {
    if destination.replace(value).is_some() {
        Err(UploadFormError::RepeatedField)
    } else {
        Ok(())
    }
}

fn parse_text(contents: &[u8]) -> Result<&str, UploadFormError> {
    if contents.is_empty() || contents.len() > MAXIMUM_TEXT_FIELD_BYTES {
        return Err(UploadFormError::InvalidField);
    }
    str::from_utf8(contents).map_err(|_| UploadFormError::InvalidField)
}

fn parse_csrf(contents: &[u8]) -> Result<String, UploadFormError> {
    let csrf = parse_text(contents)?;
    if csrf.len() != 43
        || !csrf
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(UploadFormError::InvalidField);
    }
    Ok(csrf.to_owned())
}

fn validate_filename(filename: &str) -> Result<(), UploadFormError> {
    if filename.is_empty()
        || filename.len() > MAXIMUM_FILENAME_BYTES
        || filename.contains(['/', '\\', '\0'])
        || filename.chars().any(char::is_control)
    {
        Err(UploadFormError::InvalidFilename)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UploadFormError {
    Malformed,
    UnexpectedField,
    RepeatedField,
    MissingField,
    InvalidField,
    InvalidFilename,
}

#[cfg(test)]
mod tests {
    use std::{
        fs, io,
        net::{IpAddr, SocketAddr},
        str::FromStr as _,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Instant,
    };

    use axum::{
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::{Method, Request, Response, header},
    };
    use tower::ServiceExt as _;

    use crate::{auth::LoginOutcome, catalog::load, password::PasswordConfig};

    use super::*;

    const HOST_NAME: &str = "192.0.2.10:8080";
    const SOURCE: &str = "192.0.2.40:49152";
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
        application: Arc<Application>,
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
                application: Arc::new(Application::new(
                    AuthManager::new(password),
                    roms,
                    palette,
                    Box::new(restarter),
                )),
                directory,
                restarts,
            })
        }

        fn session(&self, now: Instant) -> Option<(String, String)> {
            let source = IpAddr::from_str("192.0.2.40").ok()?;
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

    fn request(
        method: Method,
        path: &str,
        host: &str,
        headers: &[(&str, &str)],
        body: impl Into<Body>,
    ) -> Option<Request<Body>> {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(HOST, host);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let mut request = builder.body(body.into()).ok()?;
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from_str(SOURCE).ok()?));
        Some(request)
    }

    async fn send(fixture: &Fixture, request: Request<Body>) -> Option<Response<Body>> {
        Arc::clone(&fixture.application)
            .router()
            .oneshot(request)
            .await
            .ok()
    }

    async fn response_text(response: Response<Body>) -> Option<String> {
        let bytes = to_bytes(response.into_body(), 32 * 1_024 * 1_024)
            .await
            .ok()?;
        String::from_utf8(bytes.to_vec()).ok()
    }

    fn cookie_header(token: &str) -> String {
        format!("{SESSION_COOKIE_NAME}={token}")
    }

    fn origin_headers<'a>(content_type: &'a str, cookie: &'a str) -> [(&'a str, &'a str); 3] {
        [
            ("content-type", content_type),
            ("cookie", cookie),
            ("origin", "http://192.0.2.10:8080"),
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

    async fn login_token(fixture: &Fixture) -> Option<String> {
        let body = format!("password={PASSWORD}");
        let login_request = request(
            Method::POST,
            "/login",
            HOST_NAME,
            &[
                (
                    "content-type",
                    "application/x-www-form-urlencoded; charset=UTF-8",
                ),
                ("origin", "http://192.0.2.10:8080"),
            ],
            body,
        )?;
        let login = send(fixture, login_request).await?;
        assert_eq!(login.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            login.headers().get(header::LOCATION),
            Some(&HeaderValue::from_static("/"))
        );
        login
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())?
            .split(';')
            .next()
            .and_then(|pair| pair.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")))
            .map(str::to_owned)
    }

    #[tokio::test]
    async fn public_routes_use_axum_limits_and_harden_every_response() {
        let Some(fixture) = Fixture::new(false) else {
            return;
        };
        let Some(index_request) = request(Method::GET, "/", HOST_NAME, &[], Body::empty()) else {
            return;
        };
        let Some(index) = send(&fixture, index_request).await else {
            return;
        };
        assert_eq!(index.status(), StatusCode::OK);
        assert_eq!(
            index.headers().get(CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-store"))
        );
        assert_eq!(
            index.headers().get(X_FRAME_OPTIONS),
            Some(&HeaderValue::from_static("DENY"))
        );
        assert!(index.headers().contains_key(CONTENT_SECURITY_POLICY));
        let Some(index_text) = response_text(index).await else {
            return;
        };
        assert!(index_text.contains("name=\"password\""));

        let Some(asset_request) = request(
            Method::GET,
            "/assets/paper.css",
            HOST_NAME,
            &[],
            Body::empty(),
        ) else {
            return;
        };
        let Some(asset) = send(&fixture, asset_request).await else {
            return;
        };
        assert_eq!(asset.status(), StatusCode::OK);
        assert_eq!(
            asset.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("text/css; charset=utf-8"))
        );

        let Some(method_request) = request(Method::GET, "/login", HOST_NAME, &[], Body::empty())
        else {
            return;
        };
        let Some(wrong_method) = send(&fixture, method_request).await else {
            return;
        };
        assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            wrong_method.headers().get(header::ALLOW),
            Some(&HeaderValue::from_static("POST"))
        );

        let Some(host_request) =
            request(Method::GET, "/", "retrodeck.local:8080", &[], Body::empty())
        else {
            return;
        };
        let Some(wrong_host) = send(&fixture, host_request).await else {
            return;
        };
        assert_eq!(wrong_host.status(), StatusCode::MISDIRECTED_REQUEST);

        let oversized = format!("password={}", "x".repeat(LOGIN_FORM_BYTES));
        let Some(limit_request) = request(
            Method::POST,
            "/login",
            HOST_NAME,
            &[
                ("content-type", "application/x-www-form-urlencoded"),
                ("origin", "http://192.0.2.10:8080"),
            ],
            oversized,
        ) else {
            return;
        };
        let Some(limited) = send(&fixture, limit_request).await else {
            return;
        };
        assert_eq!(limited.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            limited.headers().get(CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-store"))
        );
    }

    #[tokio::test]
    async fn cross_origin_login_is_rejected() {
        let Some(fixture) = Fixture::new(false) else {
            return;
        };
        let body = format!("password={PASSWORD}");
        let Some(cross_origin_request) = request(
            Method::POST,
            "/login",
            HOST_NAME,
            &[
                ("content-type", "application/x-www-form-urlencoded"),
                ("origin", "http://198.51.100.7:8080"),
            ],
            body.clone(),
        ) else {
            return;
        };
        let Some(rejected) = send(&fixture, cross_origin_request).await else {
            return;
        };
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn login_dashboard_and_logout_round_trip() {
        let Some(fixture) = Fixture::new(false) else {
            return;
        };
        let Some(token) = login_token(&fixture).await else {
            return;
        };
        let cookie = cookie_header(&token);
        let Some(dashboard_request) = request(
            Method::GET,
            "/",
            HOST_NAME,
            &[("cookie", &cookie)],
            Body::empty(),
        ) else {
            return;
        };
        let Some(dashboard) = send(&fixture, dashboard_request).await else {
            return;
        };
        assert_eq!(dashboard.status(), StatusCode::OK);
        let Some(dashboard_text) = response_text(dashboard).await else {
            return;
        };
        assert!(dashboard_text.contains("action=\"/upload\""));

        let session = fixture.application.auth.session(&token, Instant::now());
        let Ok(Some(session)) = session else {
            return;
        };
        let logout_body = format!("csrf={}", session.csrf());
        let logout_headers = origin_headers("application/x-www-form-urlencoded", &cookie);
        let Some(logout_request) = request(
            Method::POST,
            "/logout",
            HOST_NAME,
            &logout_headers,
            logout_body,
        ) else {
            return;
        };
        let Some(logout) = send(&fixture, logout_request).await else {
            return;
        };
        assert_eq!(logout.status(), StatusCode::SEE_OTHER);
        assert!(
            logout
                .headers()
                .get(header::SET_COOKIE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("Max-Age=0"))
        );

        let Some(after_request) = request(
            Method::GET,
            "/",
            HOST_NAME,
            &[("cookie", &cookie)],
            Body::empty(),
        ) else {
            return;
        };
        let Some(after) = send(&fixture, after_request).await else {
            return;
        };
        let Some(after_text) = response_text(after).await else {
            return;
        };
        assert!(after_text.contains("name=\"password\""));
        assert!(!after_text.contains("action=\"/upload\""));
    }

    #[tokio::test]
    async fn upload_is_csrf_checked_filed_once_and_restarted() {
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
        let Some(upload_request) =
            request(Method::POST, "/upload", HOST_NAME, &headers, body.clone())
        else {
            return;
        };
        let Some(uploaded) = send(&fixture, upload_request).await else {
            return;
        };
        assert_eq!(uploaded.status(), StatusCode::OK);
        let Some(uploaded_text) = response_text(uploaded).await else {
            return;
        };
        assert!(uploaded_text.contains("validated, filed, and added"));
        assert_eq!(fixture.restart_count(), 1);
        assert_eq!(
            fs::read(fixture.directory.path().join("roms/chip8/space-racer.ch8")).ok(),
            Some(b"\x00\xe0".to_vec())
        );
        let catalog = load(&fixture.directory.path().join("uploads.tsv"));
        assert!(matches!(catalog, Ok(catalog) if catalog.len() == 1));

        let Some(duplicate_request) = request(Method::POST, "/upload", HOST_NAME, &headers, body)
        else {
            return;
        };
        let Some(duplicate) = send(&fixture, duplicate_request).await else {
            return;
        };
        assert_eq!(duplicate.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(fixture.restart_count(), 1);

        let bad_csrf = multipart(
            "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
            "Another Game",
            b"\x01",
        );
        let Some(rejected_request) =
            request(Method::POST, "/upload", HOST_NAME, &headers, bad_csrf)
        else {
            return;
        };
        let Some(rejected) = send(&fixture, rejected_request).await else {
            return;
        };
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
        assert!(
            !fixture
                .directory
                .path()
                .join("roms/chip8/another-game.ch8")
                .exists()
        );
    }

    #[tokio::test]
    async fn palette_save_survives_restart_failure() {
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
        let Some(palette_request) = request(Method::POST, "/palette", HOST_NAME, &headers, body)
        else {
            return;
        };
        let Some(response) = send(&fixture, palette_request).await else {
            return;
        };
        assert_eq!(response.status(), StatusCode::OK);
        let Some(response_text) = response_text(response).await else {
            return;
        };
        assert!(response_text.contains("will apply after the next restart"));
        assert_eq!(fixture.restart_count(), 1);
        let current = fixture.application.palette.current();
        assert!(matches!(
            current,
            Ok(fields) if fields.iter().any(|field| field.name == "background" && field.value == "#123456")
        ));
        assert!(fixture.directory.path().join("override.sexp").exists());
    }

    #[tokio::test]
    async fn unauthenticated_mutations_are_rejected() {
        let Some(fixture) = Fixture::new(false) else {
            return;
        };
        let Some(palette_request) = request(
            Method::POST,
            "/palette",
            HOST_NAME,
            &[
                ("content-type", "application/x-www-form-urlencoded"),
                ("origin", "http://192.0.2.10:8080"),
            ],
            "csrf=none",
        ) else {
            return;
        };
        let Some(palette) = send(&fixture, palette_request).await else {
            return;
        };
        assert_eq!(palette.status(), StatusCode::UNAUTHORIZED);
    }
}
