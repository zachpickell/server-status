//! Types shared by the agent (which produces them) and the dashboard
//! (which consumes them). They travel between the two as JSON.

use serde::{Deserialize, Serialize};

/// One snapshot of a server's state, served by the agent at `GET /stats`.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Snapshot {
    pub hostname: String,
    pub os: String,
    pub uptime_secs: u64,

    pub cpu_percent: f32,
    pub cpu_cores: usize,
    /// 1, 5 and 15 minute load averages.
    pub load: [f64; 3],

    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,

    pub disks: Vec<Disk>,

    /// Bytes per second, summed across physical network interfaces.
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,

    pub docker: Docker,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Disk {
    pub mount: String,
    pub used: u64,
    pub total: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Docker {
    /// False if the `docker` CLI isn't installed on the server.
    pub available: bool,
    /// Set when docker is installed but `docker ps` failed
    /// (daemon not running, permission denied, ...).
    pub error: Option<String>,
    pub containers: Vec<Container>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Container {
    pub name: String,
    pub image: String,
    /// running, exited, paused, restarting, created, ...
    pub state: String,
    /// Human readable, e.g. "Up 3 hours".
    pub status: String,
}

impl Docker {
    pub fn running(&self) -> usize {
        self.containers
            .iter()
            .filter(|c| c.state == "running")
            .count()
    }
}
