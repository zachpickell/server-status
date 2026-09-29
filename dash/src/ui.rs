//! Drawing. Everything here reads from `App` and paints the screen;
//! nothing here changes state.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Cell, Gauge, Paragraph, Row, Sparkline, Table, Wrap};

use common::Snapshot;

use crate::app::{App, Status};
use crate::format;

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, banner, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(if app.status() == Status::Down { 1 } else { 0 }),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, header, app);
    if app.status() == Status::Down {
        draw_down_banner(frame, banner, app);
    }

    match &app.snapshot {
        Some(snap) => draw_body(frame, body, app, snap),
        None => draw_waiting(frame, body, app),
    }

    draw_footer(frame, footer, app);
}

// ---------- header / footer ----------

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let (badge, color) = match app.status() {
        Status::Connecting => (" CONNECTING ", Color::Yellow),
        Status::Up => (" ● UP ", Color::Green),
        Status::Stale => (" ● NO RESPONSE ", Color::Yellow),
        Status::Down => (" ● DOWN ", Color::Red),
    };
    let badge_width = badge.chars().count() as u16;
    let [left, right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(badge_width)]).areas(area);

    let mut spans = vec![];
    if let Some(s) = &app.snapshot {
        spans.push(Span::styled(
            format!(" {} ", s.hostname),
            Style::new().fg(ACCENT).bold(),
        ));
        spans.push(Span::styled(
            format!(" {} · up {}", s.os, format::duration(secs(s.uptime_secs))),
            Style::new().fg(MUTED),
        ));
    } else {
        spans.push(Span::styled(
            " server-status",
            Style::new().fg(ACCENT).bold(),
        ));
    }
    frame.render_widget(Line::from(spans), left);
    frame.render_widget(
        Span::styled(badge, Style::new().fg(Color::Black).bg(color).bold()),
        right,
    );
}

fn draw_down_banner(frame: &mut Frame, area: Rect, app: &App) {
    let name = app
        .snapshot
        .as_ref()
        .map_or("server", |s| s.hostname.as_str());
    let since = app
        .failing_since
        .map(|t| format::duration(t.elapsed()))
        .unwrap_or_default();
    let err = app.last_error.as_deref().unwrap_or("");
    let text = format!(" {name} is not responding: down for {since} · {err}");
    frame.render_widget(
        Paragraph::new(text).style(Style::new().fg(Color::White).bg(Color::Red).bold()),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let updated = match app.last_ok {
        Some(t) => format!("updated {} ago", format::duration(t.elapsed())),
        None => "never updated".into(),
    };
    let line = Line::from(vec![
        Span::styled(" q", Style::new().fg(ACCENT).bold()),
        Span::raw(" quit  "),
        Span::styled("r", Style::new().fg(ACCENT).bold()),
        Span::raw(" refresh  "),
        Span::styled(format!("· {} · {updated}", app.url), Style::new().fg(MUTED)),
    ]);
    frame.render_widget(line, area);
}

fn draw_waiting(frame: &mut Frame, area: Rect, app: &App) {
    let msg = match &app.last_error {
        Some(e) => format!("Can't reach {}\n\n{e}", app.url),
        None => format!("Connecting to {}…", app.url),
    };
    let [_, mid, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(4),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(msg)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        mid,
    );
}

// ---------- body ----------

fn draw_body(frame: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    // When the server is down, keep showing the last known data, greyed out.
    let dim = app.status() == Status::Down;

    let disk_rows = snap.disks.len().max(1) as u16;
    let [top, middle, bottom] = Layout::vertical([
        Constraint::Length(7),
        Constraint::Length((disk_rows + 2).max(6)),
        Constraint::Min(5),
    ])
    .areas(area);

    let [cpu, mem] = Layout::horizontal([Constraint::Percentage(50); 2]).areas(top);
    let [disks, net] = Layout::horizontal([Constraint::Percentage(50); 2]).areas(middle);

    draw_cpu(frame, cpu, app, snap, dim);
    draw_memory(frame, mem, snap, dim);
    draw_disks(frame, disks, snap, dim);
    draw_network(frame, net, app, snap, dim);
    draw_docker(frame, bottom, snap, dim);
}

fn panel(title: &str, dim: bool) -> Block<'_> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(MUTED))
        .title(Span::styled(
            format!(" {title} "),
            Style::new()
                .fg(if dim { MUTED } else { ACCENT })
                .add_modifier(Modifier::BOLD),
        ))
}

/// Green under 70%, yellow under 90%, red above.
fn level_color(pct: f64, dim: bool) -> Color {
    if dim {
        MUTED
    } else if pct < 70.0 {
        Color::Green
    } else if pct < 90.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn gauge(pct: f64, label: String, dim: bool) -> Gauge<'static> {
    Gauge::default()
        .gauge_style(Style::new().fg(level_color(pct, dim)).bg(Color::Black))
        .ratio((pct / 100.0).clamp(0.0, 1.0))
        .label(label)
        .use_unicode(true)
}

fn draw_cpu(frame: &mut Frame, area: Rect, app: &App, snap: &Snapshot, dim: bool) {
    let block = panel("CPU", dim);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [bar, spark, info] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let pct = snap.cpu_percent as f64;
    frame.render_widget(gauge(pct, format!("{pct:.1}%"), dim), bar);
    frame.render_widget(
        sparkline(&app.cpu_history, spark.width, Some(100), dim),
        spark,
    );
    frame.render_widget(
        Line::styled(
            format!(
                "{} cores · load {:.2} {:.2} {:.2}",
                snap.cpu_cores, snap.load[0], snap.load[1], snap.load[2]
            ),
            Style::new().fg(MUTED),
        ),
        info,
    );
}

fn draw_memory(frame: &mut Frame, area: Rect, snap: &Snapshot, dim: bool) {
    let block = panel("Memory", dim);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [ram_label, ram_bar, _, swap_label, swap_bar] =
        Layout::vertical([Constraint::Length(1); 5]).areas(inner);

    let ram_pct = format::percent(snap.mem_used, snap.mem_total);
    frame.render_widget(
        label_line(
            "RAM",
            &format!(
                "{} / {}",
                format::bytes(snap.mem_used),
                format::bytes(snap.mem_total)
            ),
        ),
        ram_label,
    );
    frame.render_widget(gauge(ram_pct, format!("{ram_pct:.1}%"), dim), ram_bar);

    if snap.swap_total == 0 {
        frame.render_widget(label_line("Swap", "none"), swap_label);
    } else {
        let swap_pct = format::percent(snap.swap_used, snap.swap_total);
        frame.render_widget(
            label_line(
                "Swap",
                &format!(
                    "{} / {}",
                    format::bytes(snap.swap_used),
                    format::bytes(snap.swap_total)
                ),
            ),
            swap_label,
        );
        frame.render_widget(gauge(swap_pct, format!("{swap_pct:.1}%"), dim), swap_bar);
    }
}

fn draw_disks(frame: &mut Frame, area: Rect, snap: &Snapshot, dim: bool) {
    let block = panel("Disks", dim);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if snap.disks.is_empty() {
        frame.render_widget(
            Line::styled("no disks reported", Style::new().fg(MUTED)),
            inner,
        );
        return;
    }

    let rows = Layout::vertical(vec![Constraint::Length(1); snap.disks.len()]).split(inner);
    for (disk, row) in snap.disks.iter().zip(rows.iter()) {
        let [name, bar, size] = Layout::horizontal([
            Constraint::Length(14),
            Constraint::Min(6),
            Constraint::Length(22),
        ])
        .spacing(1)
        .areas(*row);

        let pct = format::percent(disk.used, disk.total);
        frame.render_widget(Line::from(truncate(&disk.mount, 14)), name);
        frame.render_widget(gauge(pct, format!("{pct:.0}%"), dim), bar);
        frame.render_widget(
            Line::styled(
                format!(
                    "{} / {}",
                    format::bytes(disk.used),
                    format::bytes(disk.total)
                ),
                Style::new().fg(MUTED),
            )
            .alignment(Alignment::Right),
            size,
        );
    }
}

fn draw_network(frame: &mut Frame, area: Rect, app: &App, snap: &Snapshot, dim: bool) {
    let block = panel("Network", dim);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [rx_label, rx_spark, tx_label, tx_spark] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);

    frame.render_widget(
        label_line("↓ in ", &format::rate(snap.net_rx_bps)),
        rx_label,
    );
    frame.render_widget(
        sparkline(&app.rx_history, rx_spark.width, None, dim),
        rx_spark,
    );
    frame.render_widget(
        label_line("↑ out", &format::rate(snap.net_tx_bps)),
        tx_label,
    );
    frame.render_widget(
        sparkline(&app.tx_history, tx_spark.width, None, dim),
        tx_spark,
    );
}

fn draw_docker(frame: &mut Frame, area: Rect, snap: &Snapshot, dim: bool) {
    let d = &snap.docker;
    let title = if d.available && d.error.is_none() {
        format!("Docker · {}/{} running", d.running(), d.containers.len())
    } else {
        "Docker".into()
    };
    let block = panel(&title, dim);

    let message = if !d.available {
        Some("docker is not installed on this server".to_string())
    } else if let Some(e) = &d.error {
        Some(e.clone())
    } else if d.containers.is_empty() {
        Some("no containers".to_string())
    } else {
        None
    };
    if let Some(msg) = message {
        frame.render_widget(
            Paragraph::new(msg)
                .style(Style::new().fg(MUTED))
                .wrap(Wrap { trim: true })
                .block(block),
            area,
        );
        return;
    }

    let rows = d.containers.iter().map(|c| {
        let color = if dim {
            MUTED
        } else {
            match c.state.as_str() {
                "running" => Color::Green,
                "paused" | "restarting" => Color::Yellow,
                "exited" | "dead" => Color::Red,
                _ => MUTED,
            }
        };
        Row::new(vec![
            Cell::from(Line::from(vec![
                Span::styled("● ", Style::new().fg(color)),
                Span::raw(c.name.clone()),
            ])),
            Cell::from(c.image.clone()).style(Style::new().fg(MUTED)),
            Cell::from(c.status.clone()).style(Style::new().fg(color)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(35),
            Constraint::Percentage(35),
            Constraint::Percentage(30),
        ],
    )
    .header(Row::new(vec!["NAME", "IMAGE", "STATUS"]).style(Style::new().fg(MUTED).bold()))
    .column_spacing(2)
    .block(block);

    frame.render_widget(table, area);
}

// ---------- helpers ----------

/// A sparkline showing the most recent `width` samples.
fn sparkline(
    history: &std::collections::VecDeque<u64>,
    width: u16,
    max: Option<u64>,
    dim: bool,
) -> Sparkline<'static> {
    let skip = history.len().saturating_sub(width as usize);
    let data: Vec<u64> = history.iter().skip(skip).copied().collect();
    let mut s = Sparkline::default()
        .data(data)
        .style(Style::new().fg(if dim { MUTED } else { ACCENT }));
    if let Some(m) = max {
        s = s.max(m);
    }
    s
}

fn label_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label} "), Style::new().fg(MUTED)),
        Span::raw(value.to_string()).bold(),
    ])
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn secs(n: u64) -> std::time::Duration {
    std::time::Duration::from_secs(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poller::PollResult;
    use common::{Container, Disk, Docker};
    use ratatui::{Terminal, backend::TestBackend};

    fn sample() -> Snapshot {
        Snapshot {
            hostname: "falc".into(),
            os: "Linux (Ubuntu 24.04)".into(),
            uptime_secs: 3 * 86400 + 4 * 3600 + 720,
            cpu_percent: 37.5,
            cpu_cores: 8,
            load: [1.2, 0.9, 0.7],
            mem_used: 10 << 30,
            mem_total: 16 << 30,
            swap_used: 0,
            swap_total: 4 << 30,
            disks: vec![
                Disk {
                    mount: "/".into(),
                    used: 120 << 30,
                    total: 460 << 30,
                },
                Disk {
                    mount: "/mnt/storage".into(),
                    used: 3400 << 30,
                    total: 3700 << 30,
                },
            ],
            net_rx_bps: 1_500_000,
            net_tx_bps: 250_000,
            docker: Docker {
                available: true,
                error: None,
                containers: vec![
                    Container {
                        name: "inbox-zero".into(),
                        image: "ghcr.io/elie222/inbox-zero".into(),
                        state: "running".into(),
                        status: "Up 3 hours".into(),
                    },
                    Container {
                        name: "old-thing".into(),
                        image: "nginx".into(),
                        state: "exited".into(),
                        status: "Exited (0) 2 days ago".into(),
                    },
                ],
            },
            ..Default::default()
        }
    }

    fn render(app: &App) -> String {
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_up_and_down() {
        let mut app = App::new("http://falc:9100/stats".into());
        for i in 0..40 {
            let mut s = sample();
            s.cpu_percent = (i * 7 % 100) as f32;
            app.apply(PollResult::Ok(s));
        }
        let up = render(&app);
        println!("{up}");
        assert!(up.contains("● UP"));
        assert!(up.contains("/mnt/storage"));
        assert!(up.contains("inbox-zero"));

        for _ in 0..3 {
            app.apply(PollResult::Err("connection refused".into()));
        }
        let down = render(&app);
        println!("{down}");
        assert!(down.contains("● DOWN"));
        assert!(down.contains("falc is not responding"));
    }

    #[test]
    fn renders_before_first_response() {
        let mut app = App::new("http://falc:9100/stats".into());
        assert!(render(&app).contains("Connecting to"));
        app.apply(PollResult::Err("connection refused".into()));
        assert!(render(&app).contains("Can't reach"));
    }
}
