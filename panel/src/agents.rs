//! Talking to server-agent instances. Tokens never leave the panel: the
//! browser asks the panel, and the panel adds the right token here.

use std::time::Duration;

use common::Snapshot;
use serde::Deserialize;

use crate::settings::ServerEntry;

const STATS_TIMEOUT: Duration = Duration::from_secs(4);
const ACTION_TIMEOUT: Duration = Duration::from_secs(75);
const LOGS_TIMEOUT: Duration = Duration::from_secs(20);

/// An error to show the user, with the HTTP status the panel should return.
pub struct AgentError {
    pub status: u16,
    pub message: String,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

pub async fn stats(http: &reqwest::Client, server: &ServerEntry) -> Result<Snapshot, AgentError> {
    let resp = send(
        http.get(format!("{}/stats", server.url)),
        server,
        STATS_TIMEOUT,
    )
    .await?;
    resp.json::<Snapshot>().await.map_err(|e| AgentError {
        status: 502,
        message: format!("bad response from agent: {e}"),
    })
}

pub async fn container_action(
    http: &reqwest::Client,
    server: &ServerEntry,
    container: &str,
    action: &str,
) -> Result<(), AgentError> {
    let url = format!("{}/containers/{container}/{action}", server.url);
    send(http.post(url), server, ACTION_TIMEOUT)
        .await
        .map(|_| ())
}

pub async fn container_logs(
    http: &reqwest::Client,
    server: &ServerEntry,
    container: &str,
    lines: u32,
) -> Result<String, AgentError> {
    let url = format!("{}/containers/{container}/logs?lines={lines}", server.url);
    let resp = send(http.get(url), server, LOGS_TIMEOUT).await?;
    resp.text().await.map_err(|e| AgentError {
        status: 502,
        message: e.to_string(),
    })
}

pub async fn reboot(http: &reqwest::Client, server: &ServerEntry) -> Result<(), AgentError> {
    let url = format!("{}/reboot", server.url);
    send(http.post(url), server, ACTION_TIMEOUT)
        .await
        .map(|_| ())
}

/// Adds the token and timeout, sends, and turns failures into readable errors.
async fn send(
    req: reqwest::RequestBuilder,
    server: &ServerEntry,
    timeout: Duration,
) -> Result<reqwest::Response, AgentError> {
    let mut req = req.timeout(timeout);
    if let Some(t) = &server.token {
        req = req.bearer_auth(t);
    }

    let resp = req.send().await.map_err(|e| AgentError {
        status: 502,
        message: if e.is_timeout() {
            "the server took too long to respond".into()
        } else if e.is_connect() {
            "can't connect to the agent (is it running, and is the address right?)".into()
        } else {
            e.to_string()
        },
    })?;

    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }

    // Pass the agent's own error message through when it sent one.
    let message = match resp.json::<ErrorBody>().await {
        Ok(b) => b.error,
        Err(_) => format!("agent returned {status}"),
    };
    let message = if status.as_u16() == 401 {
        "unauthorized: the token doesn't match this server's AGENT_TOKEN".into()
    } else {
        message
    };
    Err(AgentError {
        // Don't pass a 401 through as-is, or the browser would think *its*
        // panel login had expired.
        status: if status.as_u16() == 401 {
            502
        } else {
            status.as_u16()
        },
        message,
    })
}
