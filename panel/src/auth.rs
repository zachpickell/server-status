//! Password login and session cookies.
//!
//! - The password is stored only as an Argon2 hash.
//! - A successful login creates a random 256-bit session id, kept in memory
//!   (restarting the panel logs everyone out) and sent as an HttpOnly,
//!   SameSite=Strict cookie.
//! - Anything that changes state must also send the header `X-Panel: 1`.
//!   Browsers won't let other websites add custom headers to cross-site
//!   requests, so this blocks CSRF on top of SameSite.
//! - Too many wrong passwords in a row locks login for a few minutes.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use axum::http::HeaderMap;

pub const COOKIE: &str = "panel_session";
pub const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const MAX_FAILURES: usize = 10;
const FAILURE_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct Sessions {
    sessions: Mutex<HashMap<String, Instant>>, // id -> expires at
    failures: Mutex<Vec<Instant>>,
}

impl Sessions {
    pub fn create(&self) -> String {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("OS random number generator failed");
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

        let mut map = self.sessions.lock().unwrap();
        let now = Instant::now();
        map.retain(|_, exp| *exp > now); // drop expired sessions while we're here
        map.insert(id.clone(), now + SESSION_TTL);
        id
    }

    pub fn is_valid(&self, id: &str) -> bool {
        let map = self.sessions.lock().unwrap();
        map.get(id).is_some_and(|exp| *exp > Instant::now())
    }

    pub fn remove(&self, id: &str) {
        self.sessions.lock().unwrap().remove(id);
    }

    /// True if login is currently locked because of too many wrong passwords.
    pub fn locked_out(&self) -> bool {
        let mut f = self.failures.lock().unwrap();
        let now = Instant::now();
        f.retain(|t| now.duration_since(*t) < FAILURE_WINDOW);
        f.len() >= MAX_FAILURES
    }

    pub fn record_failure(&self) {
        self.failures.lock().unwrap().push(Instant::now());
    }

    pub fn clear_failures(&self) {
        self.failures.lock().unwrap().clear();
    }
}

pub fn hash_password(password: &str) -> Result<String, String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Reads our session id out of the Cookie header.
pub fn session_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

pub fn session_cookie(id: &str) -> String {
    format!(
        "{COOKIE}={id}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        SESSION_TTL.as_secs()
    )
}

pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let h = hash_password("correct horse").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not a hash"));
    }

    #[test]
    fn sessions() {
        let s = Sessions::default();
        let id = s.create();
        assert_eq!(id.len(), 64);
        assert!(s.is_valid(&id));
        assert!(!s.is_valid("nope"));
        s.remove(&id);
        assert!(!s.is_valid(&id));
    }

    #[test]
    fn lockout() {
        let s = Sessions::default();
        for _ in 0..MAX_FAILURES {
            assert!(!s.locked_out());
            s.record_failure();
        }
        assert!(s.locked_out());
        s.clear_failures();
        assert!(!s.locked_out());
    }

    #[test]
    fn reads_cookie() {
        let mut h = HeaderMap::new();
        h.insert("cookie", "a=1; panel_session=abc; b=2".parse().unwrap());
        assert_eq!(session_from_headers(&h).as_deref(), Some("abc"));
    }
}
