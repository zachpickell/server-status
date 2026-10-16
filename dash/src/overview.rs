//! `server-status` with no name: check every server at once and print a
//! one-line summary for each, then exit.

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::thread;
use std::time::Duration;

use common::Snapshot;

use crate::alerts;
use crate::config::Config;
use crate::format;
use crate::poller::{self, PollResult};

pub enum Row {
    Up { name: String, snap: Snapshot },
    Down { name: String, error: String },
}

/// Queries all servers in parallel so one dead server doesn't slow the rest.
fn poll_all(config: &Config) -> Vec<Row> {
    let handles: Vec<_> = config
        .servers
        .iter()
        .map(|(name, server)| {
            let name = name.clone();
            let url = crate::normalize_url(&server.url);
            let token = server.token.clone();
            thread::spawn(move || {
                let result = poller::fetch(&poller::new_agent(), &url, token.as_deref());
                match result {
                    PollResult::Ok(snap) => Row::Up { name, snap },
                    PollResult::Err(error) => Row::Down { name, error },
                }
            })
        })
        .collect();

    handles.into_iter().map(|h| h.join().unwrap()).collect()
}

/// Prints the table. Returns true if every server answered.
pub fn run(config: &Config) -> bool {
    let rows = poll_all(config);
    let all_up = rows.iter().all(|r| matches!(r, Row::Up { .. }));

    print!("{}", render(&rows, use_color()));

    if let Some(discord) = config.alert.discord.as_ref() {
        let role_id = discord.role_id.as_deref().unwrap_or("");
        for row in &rows {
            if let Row::Down { name, error } = row {
                let message = format!("<@&{role_id}>\nServer {name} is down: {error}");
                if let Err(e) = alerts::send_discord_alert(discord, &message) {
                    eprintln!("Failed to send Discord alert for {name}: {e}");
                }
            }
        }
    }

    all_up
}

/// Checks every server every `interval`, forever. Only alerts on a change
/// (down, or back up) so a server that's still down doesn't re-alert on
/// every pass.
pub fn watch(config: &Config, interval: Duration) -> ! {
    let mut down = BTreeSet::new();
    loop {
        let rows = poll_all(config);
        print!("{}", render(&rows, use_color()));

        if let Some(discord) = config.alert.discord.as_ref() {
            let role_id = discord.role_id.as_deref().unwrap_or("");
            for row in &rows {
                match row {
                    Row::Down { name, error } if down.insert(name.clone()) => {
                        let message = format!("<@&{role_id}>\nServer {name} is down: {error}");
                        if let Err(e) = alerts::send_discord_alert(discord, &message) {
                            eprintln!("Failed to send Discord alert for {name}: {e}");
                        }
                    }
                    Row::Up { name, .. } if down.remove(name) => {
                        let message = format!("<@&{role_id}>\nServer {name} is back up");
                        if let Err(e) = alerts::send_discord_alert(discord, &message) {
                            eprintln!("Failed to send Discord alert for {name}: {e}");
                        }
                    }
                    _ => {}
                }
            }
        }

        thread::sleep(interval);
    }
}

fn use_color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

pub fn render(rows: &[Row], color: bool) -> String {
    let paint = |code: &str, s: String| {
        if color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s
        }
    };

    let name_w = rows
        .iter()
        .map(|r| match r {
            Row::Up { name, .. } | Row::Down { name, .. } => name.chars().count(),
        })
        .max()
        .unwrap_or(0)
        .max(6);

    let mut out = String::new();
    out.push_str(&paint(
        "2",
        format!(
            "{:<name_w$}  {:<6}  {:>6}  {:>5}  {:<20}  {:<11}  {}",
            "SERVER", "STATUS", "CPU", "MEM", "DISK (fullest)", "UPTIME", "DOCKER"
        ),
    ));
    out.push('\n');

    for row in rows {
        match row {
            Row::Up { name, snap } => {
                let cpu = snap.cpu_percent as f64;
                let mem = format::percent(snap.mem_used, snap.mem_total);
                let disk = snap
                    .disks
                    .iter()
                    .map(|d| (format::percent(d.used, d.total), d.mount.as_str()))
                    .max_by(|a, b| a.0.total_cmp(&b.0));
                let disk_pct = disk.map_or(0.0, |d| d.0);
                let disk_text = disk.map_or("-".to_string(), |(p, m)| format!("{p:>3.0}% {m}"));
                let docker = if snap.docker.available && snap.docker.error.is_none() {
                    format!(
                        "{}/{} running",
                        snap.docker.running(),
                        snap.docker.containers.len()
                    )
                } else {
                    "-".to_string()
                };

                out.push_str(&format!("{:<name_w$}  ", name));
                out.push_str(&paint("32", format!("{:<6}", "● up")));
                out.push_str("  ");
                out.push_str(&paint(level(cpu), format!("{:>5.1}%", cpu)));
                out.push_str("  ");
                out.push_str(&paint(level(mem), format!("{:>4.0}%", mem)));
                out.push_str("  ");
                out.push_str(&paint(
                    level(disk_pct),
                    format!("{:<20}", truncate(&disk_text, 20)),
                ));
                out.push_str(&format!(
                    "  {:<11}  {}\n",
                    format::duration(std::time::Duration::from_secs(snap.uptime_secs)),
                    docker
                ));
            }
            Row::Down { name, error } => {
                out.push_str(&format!("{:<name_w$}  ", name));
                out.push_str(&paint("1;31", format!("{:<6}", "● DOWN")));
                out.push_str("  ");
                out.push_str(&paint("2", error.clone()));
                out.push('\n');
            }
        }
    }
    out
}

/// ANSI color for a usage percentage: green / yellow / red.
fn level(pct: f64) -> &'static str {
    if pct < 70.0 {
        "32"
    } else if pct < 90.0 {
        "33"
    } else {
        "31"
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max - 1).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{Disk, Docker};

    #[test]
    fn renders_up_and_down_rows() {
        let snap = Snapshot {
            cpu_percent: 12.5,
            mem_used: 8 << 30,
            mem_total: 16 << 30,
            uptime_secs: 90_000,
            disks: vec![
                Disk {
                    mount: "/".into(),
                    used: 10,
                    total: 100,
                },
                Disk {
                    mount: "/mnt/storage".into(),
                    used: 92,
                    total: 100,
                },
            ],
            docker: Docker {
                available: true,
                error: None,
                containers: vec![],
            },
            ..Default::default()
        };
        let rows = vec![
            Row::Up {
                name: "falc".into(),
                snap,
            },
            Row::Down {
                name: "talon".into(),
                error: "connection refused".into(),
            },
        ];
        let out = render(&rows, false);
        println!("{out}");
        assert!(out.contains("falc"));
        assert!(out.contains("12.5%"));
        assert!(out.contains("92% /mnt/storage"));
        assert!(out.contains("1d 1h 0m"));
        assert!(out.contains("0/0 running"));
        assert!(out.contains("talon   ● DOWN  connection refused"));
    }
}
