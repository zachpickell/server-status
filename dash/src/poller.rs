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
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .into();

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

fn fetch(agent: &ureq::Agent, url: &str, token: Option<&str>) -> PollResult {
    let mut req = agent.get(url);
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }

    let mut resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(401)) => {
            return PollResult::Err("unauthorized: check DASH_TOKEN matches AGENT_TOKEN".into());
        }
        Err(e) => return PollResult::Err(e.to_string()),
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
