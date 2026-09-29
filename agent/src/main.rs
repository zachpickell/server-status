//! server-agent: runs on each machine being monitored.
//!
//! A background thread samples system stats once per second and keeps the
//! latest snapshot in memory. A small HTTP server hands it out as JSON and
//! accepts a few control actions.
//!
//! Endpoints:
//!   GET  /health                          "ok"
//!   GET  /stats                           latest snapshot (JSON)
//!   GET  /containers/NAME/logs?lines=200  last lines of a container's logs
//!   POST /containers/NAME/start|stop|restart
//!   POST /reboot                          only if AGENT_ALLOW_REBOOT=true
//!
//! Config (environment variables):
//!   AGENT_LISTEN        address to listen on        (default 0.0.0.0:9100)
//!   AGENT_TOKEN         shared secret; requests must send
//!                       `Authorization: Bearer <token>`. Container actions
//!                       and reboot are disabled unless this is set.
//!   AGENT_ALLOW_REBOOT  "true" to enable POST /reboot (default off)

mod collect;
mod control;
mod exec;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use common::Snapshot;
use tiny_http::{Header, Method, Request, Response, Server};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// Shared between the sampler thread and every request thread.
struct Ctx {
    latest: RwLock<Snapshot>,
    token: Option<String>,
    allow_reboot: bool,
    /// Set after a container action so the next sample re-reads `docker ps`.
    docker_changed: AtomicBool,
}

fn main() {
    let listen = std::env::var("AGENT_LISTEN").unwrap_or_else(|_| "0.0.0.0:9100".into());
    let token = std::env::var("AGENT_TOKEN").ok().filter(|t| !t.is_empty());
    let allow_reboot = std::env::var("AGENT_ALLOW_REBOOT")
        .is_ok_and(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"));

    if token.is_none() {
        eprintln!(
            "warning: AGENT_TOKEN is not set: anyone who can reach {listen} can read stats, \
             and container actions/reboot are disabled"
        );
    }
    if allow_reboot && token.is_none() {
        eprintln!("warning: AGENT_ALLOW_REBOOT is ignored without AGENT_TOKEN");
    }

    let ctx = Arc::new(Ctx {
        latest: RwLock::new(Snapshot::default()),
        allow_reboot: allow_reboot && token.is_some(),
        token,
        docker_changed: AtomicBool::new(false),
    });

    // Sampler thread.
    {
        let ctx = Arc::clone(&ctx);
        thread::spawn(move || {
            let mut collector = collect::Collector::new();
            loop {
                if ctx.docker_changed.swap(false, Ordering::Relaxed) {
                    collector.refresh_docker_soon();
                }
                let mut snap = collector.sample();
                snap.can_control = ctx.token.is_some();
                snap.can_reboot = ctx.allow_reboot;
                *ctx.latest.write().unwrap() = snap;
                thread::sleep(SAMPLE_INTERVAL);
            }
        });
    }

    let server = Server::http(&listen).unwrap_or_else(|e| {
        eprintln!("error: could not listen on {listen}: {e}");
        std::process::exit(1);
    });
    println!(
        "server-agent listening on http://{listen} (controls: {}, reboot: {})",
        if ctx.token.is_some() { "on" } else { "off" },
        if ctx.allow_reboot { "on" } else { "off" },
    );

    for request in server.incoming_requests() {
        // One thread per request: a slow `docker stop` must never hold up
        // /stats, or dashboards would think the server is down.
        let ctx = Arc::clone(&ctx);
        thread::spawn(move || handle(request, &ctx));
    }
}

fn handle(request: Request, ctx: &Ctx) {
    if let Some(expected) = &ctx.token
        && !is_authorized(&request, expected)
    {
        return respond_error(request, 401, "unauthorized");
    }

    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let method = request.method().clone();

    match (&method, parts.as_slice()) {
        (Method::Get, ["health"]) => {
            let _ = request.respond(Response::from_string("ok"));
        }

        (Method::Get, ["stats"]) => {
            let body = serde_json::to_string(&*ctx.latest.read().unwrap()).unwrap();
            respond_json(request, 200, body);
        }

        (Method::Get, ["containers", name, "logs"]) => {
            if ctx.token.is_none() {
                return controls_disabled(request);
            }
            let lines = query_param(query, "lines")
                .and_then(|v| v.parse().ok())
                .unwrap_or(200);
            match control::container_logs(name, lines) {
                Ok(text) => {
                    let _ = request.respond(Response::from_string(text).with_header(
                        Header::from_bytes("Content-Type", "text/plain; charset=utf-8").unwrap(),
                    ));
                }
                Err((code, msg)) => respond_error(request, code, &msg),
            }
        }

        (Method::Post, ["containers", name, action]) => {
            if ctx.token.is_none() {
                return controls_disabled(request);
            }
            match control::container_action(name, action) {
                Ok(()) => {
                    ctx.docker_changed.store(true, Ordering::Relaxed);
                    respond_json(request, 200, r#"{"ok":true}"#.into());
                }
                Err((code, msg)) => respond_error(request, code, &msg),
            }
        }

        (Method::Post, ["reboot"]) => {
            if !ctx.allow_reboot {
                return respond_error(
                    request,
                    403,
                    "reboot is disabled on this server (set AGENT_ALLOW_REBOOT=true)",
                );
            }
            eprintln!("reboot requested");
            match control::reboot() {
                Ok(()) => respond_json(request, 200, r#"{"ok":true}"#.into()),
                Err((code, msg)) => respond_error(request, code, &msg),
            }
        }

        _ => respond_error(request, 404, "not found"),
    }
}

fn is_authorized(request: &Request, expected: &str) -> bool {
    let wanted = format!("Bearer {expected}");
    request
        .headers()
        .iter()
        .any(|h| h.field.equiv("Authorization") && h.value.as_str() == wanted)
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

fn respond_json(request: Request, code: u16, body: String) {
    let _ = request.respond(
        Response::from_string(body)
            .with_status_code(code)
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
    );
}

fn respond_error(request: Request, code: u16, msg: &str) {
    let body = serde_json::json!({ "error": msg }).to_string();
    respond_json(request, code, body);
}

fn controls_disabled(request: Request) {
    respond_error(
        request,
        403,
        "controls are disabled on this server (set AGENT_TOKEN to enable them)",
    );
}

#[cfg(test)]
mod tests {
    use super::query_param;

    #[test]
    fn query() {
        assert_eq!(query_param("lines=50&x=1", "lines"), Some("50"));
        assert_eq!(query_param("x=1", "lines"), None);
        assert_eq!(query_param("", "lines"), None);
    }
}
