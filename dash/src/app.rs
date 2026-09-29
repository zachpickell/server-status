//! Dashboard state: the latest snapshot, history for the sparklines, and
//! whether the server is up or down.

use std::collections::VecDeque;
use std::time::Instant;

use common::Snapshot;

use crate::poller::PollResult;

/// How many failed polls in a row before we call the server DOWN.
pub const DOWN_AFTER_FAILURES: u32 = 3;
const HISTORY_LEN: usize = 300;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Connecting, // haven't heard anything yet
    Up,
    Stale, // recent poll(s) failed, but not enough to call it down
    Down,
}

pub struct App {
    pub url: String,
    pub snapshot: Option<Snapshot>,
    pub last_ok: Option<Instant>,
    pub last_error: Option<String>,
    pub failures: u32,
    /// When the current run of failures started.
    pub failing_since: Option<Instant>,

    pub cpu_history: VecDeque<u64>,
    pub rx_history: VecDeque<u64>,
    pub tx_history: VecDeque<u64>,
}

impl App {
    pub fn new(url: String) -> Self {
        Self {
            url,
            snapshot: None,
            last_ok: None,
            last_error: None,
            failures: 0,
            failing_since: None,
            cpu_history: VecDeque::new(),
            rx_history: VecDeque::new(),
            tx_history: VecDeque::new(),
        }
    }

    pub fn apply(&mut self, result: PollResult) {
        match result {
            PollResult::Ok(snap) => {
                push(&mut self.cpu_history, snap.cpu_percent.round() as u64);
                push(&mut self.rx_history, snap.net_rx_bps);
                push(&mut self.tx_history, snap.net_tx_bps);
                self.snapshot = Some(snap);
                self.last_ok = Some(Instant::now());
                self.last_error = None;
                self.failures = 0;
                self.failing_since = None;
            }
            PollResult::Err(e) => {
                self.last_error = Some(e);
                self.failures += 1;
                self.failing_since.get_or_insert_with(Instant::now);
            }
        }
    }

    pub fn status(&self) -> Status {
        if self.failures >= DOWN_AFTER_FAILURES {
            Status::Down
        } else if self.failures > 0 {
            Status::Stale
        } else if self.snapshot.is_some() {
            Status::Up
        } else {
            Status::Connecting
        }
    }
}

fn push(buf: &mut VecDeque<u64>, v: u64) {
    buf.push_back(v);
    while buf.len() > HISTORY_LEN {
        buf.pop_front();
    }
}
