//! server-dash: terminal dashboard that runs on the VPS and watches falc.
//!
//! Usage:
//!   server-dash [URL]
//!
//! URL defaults to $DASH_URL, then http://127.0.0.1:9100. "/stats" is added
//! if missing. Set DASH_TOKEN to the same value as the agent's AGENT_TOKEN.
//!
//! Keys: q / Esc quit, r refresh now.

mod app;
mod format;
mod poller;
mod ui;

use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use app::App;
use poller::PollResult;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const UI_TICK: Duration = Duration::from_millis(250);

fn main() -> io::Result<()> {
    let arg = std::env::args().nth(1);
    if matches!(arg.as_deref(), Some("-h" | "--help")) {
        println!("usage: server-dash [URL]\n\nenv: DASH_URL, DASH_TOKEN\nkeys: q quit, r refresh");
        return Ok(());
    }

    let url = normalize_url(
        &arg.or_else(|| std::env::var("DASH_URL").ok())
            .unwrap_or_else(|| "http://127.0.0.1:9100".into()),
    );
    let token = std::env::var("DASH_TOKEN").ok().filter(|t| !t.is_empty());

    let (refresh_tx, refresh_rx) = mpsc::channel();
    let results = poller::spawn(url.clone(), token, POLL_INTERVAL, refresh_rx);
    let mut app = App::new(url);

    // ratatui::init switches to the alternate screen + raw mode and installs
    // a panic hook that restores the terminal if we crash.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &results, &refresh_tx);
    ratatui::restore();
    result
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

fn normalize_url(url: &str) -> String {
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
