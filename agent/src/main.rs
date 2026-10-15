//! server-agent: runs on the machine being monitored (falc).
//!
//! A background thread samples system stats once per second and keeps the
//! latest snapshot in memory. A tiny HTTP server hands that snapshot out as
//! JSON, so every request is instant and cheap.
//!
//! Config (environment variables):
//!   AGENT_LISTEN  address to listen on        (default 0.0.0.0:9100)
//!   AGENT_TOKEN   shared secret; if set, requests must send
//!                 `Authorization: Bearer <token>`

mod collect;

use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use common::Snapshot;
use tiny_http::{Header, Response, Server};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

fn main() {
    let listen = std::env::var("AGENT_LISTEN").unwrap_or_else(|_| "0.0.0.0:9100".into());
    let token = std::env::var("AGENT_TOKEN").ok().filter(|t| !t.is_empty());

    if token.is_none() {
        eprintln!("warning: AGENT_TOKEN is not set, anyone who can reach {listen} can read stats");
    }

    // Latest snapshot, shared between the sampler thread and the HTTP server.
    let latest = Arc::new(RwLock::new(Snapshot::default()));

    // Sampler thread.
    {
        let latest = Arc::clone(&latest);
        thread::spawn(move || {
            let mut collector = collect::Collector::new();
            loop {
                let snap = collector.sample();
                *latest.write().unwrap() = snap;
                thread::sleep(SAMPLE_INTERVAL);
            }
        });
    }

    let server = Server::http(&listen).unwrap_or_else(|e| {
        eprintln!("error: could not listen on {listen}: {e}");
        std::process::exit(1);
    });
    println!("server-agent listening on http://{listen}");

    let json_header = Header::from_bytes("Content-Type", "application/json").unwrap();

    for request in server.incoming_requests() {
        if let Some(expected) = &token
            && !is_authorized(&request, expected)
        {
            let _ = request.respond(Response::from_string("unauthorized").with_status_code(401));
            continue;
        }

        let response = match request.url() {
            "/stats" => {
                let body = serde_json::to_string(&*latest.read().unwrap()).unwrap();
                Response::from_string(body).with_header(json_header.clone())
            }
            "/health" => Response::from_string("ok"),
            _ => Response::from_string("not found").with_status_code(404),
        };
        let _ = request.respond(response);
    }
}

fn is_authorized(request: &tiny_http::Request, expected: &str) -> bool {
    request
        .headers()
        .iter()
        .any(|h| h.field.equiv("Authorization") && h.value.as_str() == format!("Bearer {expected}"))
}
