//! Background thread that fetches `/stats` from the agent on a timer and
//! sends each result back to the UI thread over a channel. Keeping network
//! calls off the UI thread means a dead server never freezes the screen.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use common::Snapshot;

pub enum PollResult {
    Ok(Snapshot),
    Err(String),
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

/// Starts the poller. Send `()` on `refresh` to fetch immediately.
pub fn spawn(
    url: String,
    token: Option<String>,
    interval: Duration,
    refresh: Receiver<()>,
) -> Receiver<PollResult> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let agent = new_agent();

        loop {
            let result = fetch(&agent, &url, token.as_deref());
            if tx.send(result).is_err() {
                return; // UI has exited
            }
            // Sleep until the next poll, or wake early on a manual refresh.
            match refresh.recv_timeout(interval) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });

    rx
}

/// An HTTP client with a timeout, so a dead server fails fast.
pub fn new_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into()
}

/// Fetch one snapshot. Used by the poller loop and by the overview.
pub fn fetch(agent: &ureq::Agent, url: &str, token: Option<&str>) -> PollResult {
    let mut req = agent.get(url);
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }

    let mut resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(401)) => {
            return PollResult::Err(
                "unauthorized: token doesn't match the agent's AGENT_TOKEN".into(),
            );
        }
        Err(e) => return PollResult::Err(tidy_error(&e.to_string())),
    };

    let body = match resp.body_mut().read_to_string() {
        Ok(b) => b,
        Err(e) => return PollResult::Err(e.to_string()),
    };

    match serde_json::from_str::<Snapshot>(&body) {
        Ok(s) => PollResult::Ok(s),
        Err(e) => PollResult::Err(format!("bad response from agent: {e}")),
    }
}

/// "io: Connection refused (os error 111)" -> "connection refused"
fn tidy_error(msg: &str) -> String {
    let msg = msg.strip_prefix("io: ").unwrap_or(msg);
    let msg = match msg.find(" (os error") {
        Some(i) => &msg[..i],
        None => msg,
    };
    let mut chars = msg.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tidies_errors() {
        assert_eq!(
            super::tidy_error("io: Connection refused (os error 111)"),
            "connection refused"
        );
        assert_eq!(super::tidy_error("timeout: global"), "timeout: global");
    }
}
