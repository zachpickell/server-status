//! server-panel: a small web page (meant to run on talon) for watching and
//! controlling every server that runs server-agent.
//!
//! Usage:
//!   server-panel [--config PATH]   run the web server
//!                                  (default /etc/server-panel/panel.toml,
//!                                   or $PANEL_CONFIG)
//!   server-panel hash-password     make a password_hash for panel.toml

mod agents;
mod api;
mod auth;
mod settings;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::middleware;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post, put};
use tokio::sync::RwLock;

use api::AppState;

const INDEX_HTML: &str = include_str!("../static/index.html");

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("hash-password") => return hash_password_cmd(),
        Some("-h" | "--help") => {
            println!(
                "usage:\n  server-panel [--config PATH]\n  server-panel hash-password\n\n\
                 default config: /etc/server-panel/panel.toml (or $PANEL_CONFIG)"
            );
            return;
        }
        _ => {}
    }

    let config_path = match args.as_slice() {
        [flag, path] if flag == "--config" => PathBuf::from(path),
        [] => std::env::var_os("PANEL_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/etc/server-panel/panel.toml")),
        _ => exit("unexpected arguments (try --help)"),
    };

    let config = settings::load_config(&config_path).unwrap_or_else(|e| exit(&e));
    let servers_path = settings::servers_file(&config, &config_path);
    let servers = settings::load_servers(&servers_path).unwrap_or_else(|e| exit(&e));

    let state = Arc::new(AppState {
        password_hash: config.password_hash,
        sessions: auth::Sessions::default(),
        servers: RwLock::new(servers),
        servers_path: servers_path.clone(),
        http: reqwest::Client::new(),
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/login", post(api::login))
        .route("/api/logout", post(api::logout))
        .route("/api/me", get(api::me))
        .route("/api/servers", get(api::list_servers))
        .route(
            "/api/servers/{name}",
            put(api::put_server).delete(api::delete_server),
        )
        .route("/api/servers/{name}/reboot", post(api::reboot))
        .route(
            "/api/servers/{name}/containers/{container}/logs",
            get(api::container_logs),
        )
        .route(
            "/api/servers/{name}/containers/{container}/{action}",
            post(api::container_action),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api::require_login,
        ))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.listen)
        .await
        .unwrap_or_else(|e| exit(&format!("could not listen on {}: {e}", config.listen)));
    println!(
        "server-panel on http://{} (servers saved in {})",
        config.listen,
        servers_path.display()
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .unwrap_or_else(|e| exit(&e.to_string()));
}

async fn index() -> impl IntoResponse {
    (
        [
            // The page is self-contained: block anything it doesn't need.
            (
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(
                    "default-src 'none'; script-src 'self' 'unsafe-inline'; \
                     style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
                     connect-src 'self'; frame-ancestors 'none'; form-action 'self'",
                ),
            ),
            (header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        Html(INDEX_HTML),
    )
}

fn hash_password_cmd() {
    let pw =
        rpassword::prompt_password("New panel password: ").unwrap_or_else(|e| exit(&e.to_string()));
    if pw.chars().count() < 8 {
        exit("use at least 8 characters");
    }
    let again = rpassword::prompt_password("Again: ").unwrap_or_else(|e| exit(&e.to_string()));
    if pw != again {
        exit("passwords didn't match");
    }
    let hash = auth::hash_password(&pw).unwrap_or_else(|e| exit(&e));
    println!("\nPut this line in /etc/server-panel/panel.toml:\n\npassword_hash = '{hash}'");
}

fn exit(msg: &str) -> ! {
    eprintln!("server-panel: {msg}");
    std::process::exit(1)
}
