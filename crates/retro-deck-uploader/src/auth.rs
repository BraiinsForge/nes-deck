//! Bounded login throttling and in-memory web sessions.

use std::{
    collections::HashMap,
    fmt,
    net::IpAddr,
    sync::{Mutex, TryLockError},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;

use crate::password::PasswordConfig;

const TOKEN_BYTES: usize = 32;
const TOKEN_CHARACTERS: usize = 43;
const SESSION_MAX_AGE_SECONDS: u32 = 8 * 60 * 60;
const SESSION_LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);
const MAXIMUM_SESSIONS: usize = 8;
const MAXIMUM_LOGIN_SOURCES: usize = 256;
const FAILURE_LIMIT: u8 = 5;
const LOCKOUT_DURATION: Duration = Duration::from_secs(5 * 60);

type SessionKey = [u8; 32];

#[derive(Debug)]
struct SessionRecord {
    csrf: String,
    expires_at: Instant,
}

#[derive(Clone, Copy, Debug)]
struct LoginAttempt {
    failures: u8,
    locked_until: Option<Instant>,
    last_seen: Instant,
}

#[derive(Debug, Default)]
struct AuthState {
    sessions: HashMap<SessionKey, SessionRecord>,
    attempts: HashMap<IpAddr, LoginAttempt>,
}

/// Authentication state with a single nonblocking password-derivation gate.
pub struct AuthManager {
    password: PasswordConfig,
    state: Mutex<AuthState>,
    verification_gate: Mutex<()>,
}

impl AuthManager {
    /// Construct empty session and attempt maps around an installed verifier.
    #[must_use]
    pub fn new(password: PasswordConfig) -> Self {
        Self {
            password,
            state: Mutex::new(AuthState::default()),
            verification_gate: Mutex::new(()),
        }
    }

    /// Check one password and create a session when it is accepted.
    ///
    /// Only one expensive PBKDF2 derivation may run at a time. A concurrent
    /// attempt receives [`LoginOutcome::Busy`] immediately instead of building
    /// an unbounded CPU queue.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError`] if a mutex was poisoned, entropy was unavailable,
    /// or the monotonic session deadline cannot be represented.
    pub fn login(
        &self,
        source: IpAddr,
        candidate: &str,
        now: Instant,
    ) -> Result<LoginOutcome, AuthError> {
        if let Some(remaining) = self.lockout_remaining(source, now)? {
            return Ok(LoginOutcome::Blocked(remaining));
        }
        let _verification = match self.verification_gate.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Ok(LoginOutcome::Busy),
            Err(TryLockError::Poisoned(_)) => return Err(AuthError::LockPoisoned),
        };
        if let Some(remaining) = self.lockout_remaining(source, now)? {
            return Ok(LoginOutcome::Blocked(remaining));
        }

        let accepted = self.password.matches(candidate);
        self.record_login(source, accepted, now)?;
        if !accepted {
            return Ok(LoginOutcome::Rejected);
        }
        self.create_session(now).map(LoginOutcome::Accepted)
    }

    /// Resolve one canonical cookie token to a live session handle.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::LockPoisoned`] when a prior panic poisoned state.
    pub fn session(&self, token: &str, now: Instant) -> Result<Option<Session>, AuthError> {
        let Some(key) = session_key(token) else {
            return Ok(None);
        };
        let mut state = self.state.lock().map_err(|_| AuthError::LockPoisoned)?;
        let Some(record) = state.sessions.get(&key) else {
            return Ok(None);
        };
        if record.expires_at <= now {
            state.sessions.remove(&key);
            return Ok(None);
        }
        Ok(Some(Session {
            key,
            csrf: record.csrf.clone(),
        }))
    }

    /// End a live session only when its CSRF value matches in constant time.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::LockPoisoned`] when a prior panic poisoned state.
    pub fn logout(
        &self,
        session: &Session,
        provided_csrf: &str,
        now: Instant,
    ) -> Result<bool, AuthError> {
        let mut state = self.state.lock().map_err(|_| AuthError::LockPoisoned)?;
        let Some(record) = state.sessions.get(&session.key) else {
            return Ok(false);
        };
        if record.expires_at <= now {
            state.sessions.remove(&session.key);
            return Ok(false);
        }
        if !constant_time_equal(record.csrf.as_bytes(), provided_csrf.as_bytes()) {
            return Ok(false);
        }
        state.sessions.remove(&session.key);
        Ok(true)
    }

    fn lockout_remaining(
        &self,
        source: IpAddr,
        now: Instant,
    ) -> Result<Option<Duration>, AuthError> {
        let mut state = self.state.lock().map_err(|_| AuthError::LockPoisoned)?;
        let Some(attempt) = state.attempts.get_mut(&source) else {
            return Ok(None);
        };
        match attempt.locked_until {
            Some(deadline) if deadline > now => Ok(Some(deadline.duration_since(now))),
            Some(_) => {
                attempt.locked_until = None;
                Ok(None)
            }
            None => Ok(None),
        }
    }

    fn record_login(&self, source: IpAddr, accepted: bool, now: Instant) -> Result<(), AuthError> {
        let mut state = self.state.lock().map_err(|_| AuthError::LockPoisoned)?;
        if accepted {
            state.attempts.remove(&source);
            return Ok(());
        }
        if !state.attempts.contains_key(&source) && state.attempts.len() >= MAXIMUM_LOGIN_SOURCES {
            if let Some(oldest) = state
                .attempts
                .iter()
                .min_by_key(|(_, attempt)| attempt.last_seen)
                .map(|(address, _)| *address)
            {
                state.attempts.remove(&oldest);
            }
        }
        let attempt = state.attempts.entry(source).or_insert(LoginAttempt {
            failures: 0,
            locked_until: None,
            last_seen: now,
        });
        attempt.failures = attempt.failures.saturating_add(1);
        attempt.last_seen = now;
        if attempt.failures >= FAILURE_LIMIT {
            attempt.failures = 0;
            attempt.locked_until = now.checked_add(LOCKOUT_DURATION);
            if attempt.locked_until.is_none() {
                return Err(AuthError::TimeOverflow);
            }
        }
        Ok(())
    }

    fn create_session(&self, now: Instant) -> Result<SessionCookie, AuthError> {
        let token = random_token()?;
        let csrf = random_token()?;
        let key = session_key(&token).ok_or(AuthError::InvalidGeneratedToken)?;
        let expires_at = now
            .checked_add(SESSION_LIFETIME)
            .ok_or(AuthError::TimeOverflow)?;
        let mut state = self.state.lock().map_err(|_| AuthError::LockPoisoned)?;
        state.sessions.retain(|_, session| session.expires_at > now);
        if state.sessions.len() >= MAXIMUM_SESSIONS {
            if let Some(oldest) = state
                .sessions
                .iter()
                .min_by_key(|(_, session)| session.expires_at)
                .map(|(key, _)| *key)
            {
                state.sessions.remove(&oldest);
            }
        }
        state
            .sessions
            .insert(key, SessionRecord { csrf, expires_at });
        Ok(SessionCookie { token })
    }
}

impl fmt::Debug for AuthManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthManager")
            .field("password", &self.password)
            .finish_non_exhaustive()
    }
}

/// Result of one bounded login attempt.
#[derive(Debug)]
pub enum LoginOutcome {
    /// The password was accepted and a session cookie was created.
    Accepted(SessionCookie),
    /// The password was not accepted.
    Rejected,
    /// This source reached the failure limit and must wait.
    Blocked(Duration),
    /// Another PBKDF2 verification is already using the single CPU gate.
    Busy,
}

/// Newly issued opaque session cookie value.
pub struct SessionCookie {
    token: String,
}

impl SessionCookie {
    /// Return the canonical base64url cookie value.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Return the browser cookie lifetime in seconds.
    #[must_use]
    pub const fn max_age_seconds(&self) -> u32 {
        SESSION_MAX_AGE_SECONDS
    }
}

impl fmt::Debug for SessionCookie {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionCookie")
            .field("token", &"[redacted]")
            .field("max_age_seconds", &self.max_age_seconds())
            .finish()
    }
}

/// Opaque handle to a live authenticated session.
pub struct Session {
    key: SessionKey,
    csrf: String,
}

impl Session {
    /// Return the unguessable value required on mutating forms.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn csrf(&self) -> &str {
        &self.csrf
    }

    /// Compare a submitted CSRF value in constant time.
    #[must_use]
    pub fn csrf_matches(&self, candidate: &str) -> bool {
        constant_time_equal(self.csrf.as_bytes(), candidate.as_bytes())
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("key", &"[redacted]")
            .field("csrf", &"[redacted]")
            .finish()
    }
}

/// Session entropy, timing, or synchronization failure.
#[derive(Debug)]
pub enum AuthError {
    /// Operating-system entropy was unavailable.
    Random(String),
    /// An internally generated token violated its own encoding contract.
    InvalidGeneratedToken,
    /// A monotonic expiration deadline cannot be represented.
    TimeOverflow,
    /// A prior panic poisoned authentication state.
    LockPoisoned,
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random(error) => write!(formatter, "cannot generate session entropy: {error}"),
            Self::InvalidGeneratedToken => {
                formatter.write_str("generated session token is invalid")
            }
            Self::TimeOverflow => formatter.write_str("session deadline is out of range"),
            Self::LockPoisoned => formatter.write_str("authentication lock was poisoned"),
        }
    }
}

impl std::error::Error for AuthError {}

fn random_token() -> Result<String, AuthError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::getrandom(&mut bytes).map_err(|error| AuthError::Random(error.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn session_key(token: &str) -> Option<SessionKey> {
    if token.len() != TOKEN_CHARACTERS {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(token).ok()?;
    if decoded.len() != TOKEN_BYTES || URL_SAFE_NO_PAD.encode(&decoded) != token {
        return None;
    }
    Some(Sha256::digest(token.as_bytes()).into())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && bool::from(left.ct_eq(right))
}

#[cfg(test)]
mod tests {
    use super::{AuthManager, LOCKOUT_DURATION, LoginOutcome, MAXIMUM_SESSIONS, SESSION_LIFETIME};
    use crate::password::PasswordConfig;
    use std::{net::IpAddr, str::FromStr as _, time::Instant};

    fn manager() -> Option<AuthManager> {
        PasswordConfig::new("configured-password")
            .ok()
            .map(AuthManager::new)
    }

    fn source() -> Option<IpAddr> {
        IpAddr::from_str("192.0.2.40").ok()
    }

    #[test]
    fn accepted_login_creates_and_revokes_a_hardened_session() {
        let Some(manager) = manager() else {
            return;
        };
        let Some(source) = source() else {
            return;
        };
        let now = Instant::now();
        let outcome = manager.login(source, "configured-password", now);
        assert!(matches!(outcome, Ok(LoginOutcome::Accepted(_))));
        let Ok(LoginOutcome::Accepted(cookie)) = outcome else {
            return;
        };
        assert_eq!(cookie.token().len(), 43);
        let session = manager.session(cookie.token(), now);
        assert!(matches!(session, Ok(Some(_))));
        let Ok(Some(session)) = session else {
            return;
        };
        assert_eq!(session.csrf().len(), 43);
        assert!(session.csrf_matches(session.csrf()));
        assert!(!session.csrf_matches("wrong"));
        assert!(matches!(manager.logout(&session, "wrong", now), Ok(false)));
        assert!(matches!(
            manager.logout(&session, session.csrf(), now),
            Ok(true)
        ));
        assert!(matches!(manager.session(cookie.token(), now), Ok(None)));
    }

    #[test]
    fn fifth_failure_locks_the_source_without_running_another_derivation() {
        let Some(manager) = manager() else {
            return;
        };
        let Some(source) = source() else {
            return;
        };
        let now = Instant::now();
        for _ in 0..5 {
            assert!(manager.record_login(source, false, now).is_ok());
        }
        assert!(matches!(
            manager.login(source, "configured-password", now),
            Ok(LoginOutcome::Blocked(remaining)) if remaining == LOCKOUT_DURATION
        ));
        let later = now.checked_add(LOCKOUT_DURATION);
        assert!(later.is_some());
        let Some(later) = later else {
            return;
        };
        assert!(matches!(
            manager.login(source, "configured-password", later),
            Ok(LoginOutcome::Accepted(_))
        ));
    }

    #[test]
    fn concurrent_derivation_fails_fast_as_busy() {
        let Some(manager) = manager() else {
            return;
        };
        let Some(source) = source() else {
            return;
        };
        let gate = manager.verification_gate.lock();
        assert!(gate.is_ok());
        let Some(_gate) = gate.ok() else {
            return;
        };
        assert!(matches!(
            manager.login(source, "configured-password", Instant::now()),
            Ok(LoginOutcome::Busy)
        ));
    }

    #[test]
    fn sessions_are_bounded_and_expire_on_monotonic_time() {
        let Some(manager) = manager() else {
            return;
        };
        let now = Instant::now();
        let mut cookies = Vec::new();
        for offset in 0..=MAXIMUM_SESSIONS {
            let created_at = now.checked_add(std::time::Duration::from_secs(
                u64::try_from(offset).unwrap_or(u64::MAX),
            ));
            assert!(created_at.is_some());
            let Some(created_at) = created_at else {
                return;
            };
            let cookie = manager.create_session(created_at);
            assert!(cookie.is_ok());
            let Some(cookie) = cookie.ok() else {
                return;
            };
            cookies.push(cookie);
        }
        let Some(oldest) = cookies.first() else {
            return;
        };
        assert!(matches!(manager.session(oldest.token(), now), Ok(None)));
        let Some(newest) = cookies.last() else {
            return;
        };
        assert!(matches!(manager.session(newest.token(), now), Ok(Some(_))));
        let expired = now.checked_add(SESSION_LIFETIME + std::time::Duration::from_secs(60));
        assert!(expired.is_some());
        let Some(expired) = expired else {
            return;
        };
        assert!(matches!(manager.session(newest.token(), expired), Ok(None)));
    }

    #[test]
    fn malformed_cookie_tokens_never_reach_the_session_map() {
        let Some(manager) = manager() else {
            return;
        };
        let now = Instant::now();
        for token in ["", "short", "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"] {
            assert!(matches!(manager.session(token, now), Ok(None)));
        }
    }
}
