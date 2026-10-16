//! server-status: check on your servers from any machine on your Tailscale network.
//!
//! Usage:
//!   server-status            one-line summary of every server in the config
//!   server-status falc       live dashboard for one server (`-falc` works too)
//!   server-status HOST:PORT  dashboard for an address not in the config
//!                            (token from $DASH_TOKEN)
//!   server-status watch      keep checking every server and alert on change
//!                            (interval set by `alert.interval_secs` in config)
//!
//! Servers are listed in ~/.config/server-status/config.toml (see config.rs).
//! Dashboard keys: q / Esc quit, r refresh now.

mod alerts;
mod app;
mod config;
mod format;
mod overview;
mod poller;
mod ui;

use std::io;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use app::App;
use poller::PollResult;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const UI_TICK: Duration = Duration::from_millis(250);

const HELP: &str = "\
usage:
  server-status            summary of every server in your config
  server-status NAME       live dashboard for one server (e.g. falc, or -falc)
  server-status HOST:PORT  dashboard for an address not in your config
                           (token from DASH_TOKEN)
  server-status watch      keep checking every server, alert only on change

config: ~/.config/server-status/config.toml
keys:   q quit, r refresh";

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);

    match arg.as_deref() {
        Some("-h" | "--help" | "help") => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("server-status {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }

        // Keep checking every server in the background and alert on change,
        // instead of only finding out a server is down when you happen to run us.
        Some("watch") => {
            let (config, _) = match config::load() {
                Ok(c) => c,
                Err(e) => return fail(&e),
            };
            if config.servers.is_empty() {
                return fail(&format!(
                    "your config has no servers yet. Add some like this:\n\n{}",
                    config::EXAMPLE
                ));
            }
            overview::watch(&config, Duration::from_secs(config.alert.interval_secs))
        }

        // No name: overview of every server.
        None => {
            let config = match config::load() {
                Ok((c, _)) => c,
                Err(e) => return fail(&e),
            };
            if config.servers.is_empty() {
                return fail(&format!(
                    "your config has no servers yet. Add some like this:\n\n{}",
                    config::EXAMPLE
                ));
            }
            if overview::run(&config) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1) // lets scripts check "is anything down?"
            }
        }

        Some(target) => {
            // Accept `falc`, `-falc` and `--falc`.
            let target = target.trim_start_matches('-');

            // Looks like an address rather than a name: connect directly.
            if target.contains(':') || target.contains('/') {
                let token = std::env::var("DASH_TOKEN").ok().filter(|t| !t.is_empty());
                return dashboard(normalize_url(target), token, None);
            }

            let (config, path) = match config::load() {
                Ok(c) => c,
                Err(e) => return fail(&e),
            };
            match config.servers.get(target) {
                Some(server) => dashboard(
                    normalize_url(&server.url),
                    server.token.clone(),
                    config.alert.discord.clone(),
                ),
                None => {
                    let names: Vec<&str> = config.servers.keys().map(String::as_str).collect();
                    fail(&format!(
                        "no server called '{target}' in {}\nservers you have: {}",
                        path.display(),
                        if names.is_empty() {
                            "(none)".to_string()
                        } else {
                            names.join(", ")
                        }
                    ))
                }
            }
        }
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("server-status: {msg}");
    ExitCode::from(2)
}

/// Open the live dashboard for one server.
fn dashboard(url: String, token: Option<String>, discord: Option<config::Discord>) -> ExitCode {
    let (refresh_tx, refresh_rx) = mpsc::channel();
    let results = poller::spawn(url.clone(), token, POLL_INTERVAL, refresh_rx);
    let mut app = App::new(url, discord);

    // ratatui::init switches to the alternate screen + raw mode and installs
    // a panic hook that restores the terminal if we crash.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &results, &refresh_tx);
    ratatui::restore();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e.to_string()),
    }
}

fn run(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    results: &Receiver<PollResult>,
    refresh: &Sender<()>,
) -> io::Result<()> {
    loop {
        // Take in everything the poller has sent since last frame.
        while let Ok(r) = results.try_recv() {
            app.apply(r);
        }

        terminal.draw(|frame| ui::draw(frame, app))?;

        // Wait briefly for a key press; redraw either way so the
        // "down for …" timers keep counting.
        if event::poll(UI_TICK)?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(());
                }
                KeyCode::Char('r') => {
                    let _ = refresh.send(());
                }
                _ => {}
            }
        }
    }
}

pub fn normalize_url(url: &str) -> String {
    let mut url = url.trim_end_matches('/').to_string();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("http://{url}");
    }
    if !url.ends_with("/stats") {
        url.push_str("/stats");
    }
    url
}

#[cfg(test)]
mod tests {
    use super::normalize_url;

    #[test]
    fn urls() {
        assert_eq!(normalize_url("falc:9100"), "http://falc:9100/stats");
        assert_eq!(
            normalize_url("http://100.64.0.2:9100/"),
            "http://100.64.0.2:9100/stats"
        );
        assert_eq!(normalize_url("http://x:1/stats"), "http://x:1/stats");
    }
}
