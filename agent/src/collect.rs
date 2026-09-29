//! Reads system stats with the `sysinfo` crate, and Docker containers
//! by running the `docker` CLI.

use std::time::{Duration, Instant};

use common::{Container, Disk, Docker, Snapshot};
use sysinfo::{Disks, Networks, System};

use crate::exec::{self, RunError};

/// Docker is slower to query than /proc, so only refresh it every few samples.
const DOCKER_EVERY: Duration = Duration::from_secs(5);
const DOCKER_TIMEOUT: Duration = Duration::from_secs(3);

pub struct Collector {
    sys: System,
    disks: Disks,
    networks: Networks,
    last_net_refresh: Instant,
    docker: Docker,
    last_docker: Option<Instant>,
}

impl Collector {
    pub fn new() -> Self {
        let mut sys = System::new();
        // CPU usage is measured between two refreshes, so prime it once.
        sys.refresh_cpu_usage();
        Self {
            sys,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            last_net_refresh: Instant::now(),
            docker: Docker::default(),
            last_docker: None,
        }
    }

    /// Re-read the container list on the next sample instead of waiting for
    /// the usual interval (called after a container is started/stopped).
    pub fn refresh_docker_soon(&mut self) {
        self.last_docker = None;
    }

    pub fn sample(&mut self) -> Snapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.disks.refresh(true);

        // Network counters are "bytes since last refresh", so divide by the
        // time since the last refresh to get bytes per second.
        self.networks.refresh(true);
        let elapsed = self.last_net_refresh.elapsed().as_secs_f64().max(0.001);
        self.last_net_refresh = Instant::now();
        let (mut rx, mut tx) = (0u64, 0u64);
        for (name, data) in &self.networks {
            if is_physical_interface(name) {
                rx += data.received();
                tx += data.transmitted();
            }
        }

        if self.last_docker.is_none_or(|t| t.elapsed() >= DOCKER_EVERY) {
            self.docker = collect_docker();
            self.last_docker = Some(Instant::now());
        }

        let load = System::load_average();

        Snapshot {
            hostname: System::host_name().unwrap_or_else(|| "unknown".into()),
            os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into()),
            uptime_secs: System::uptime(),
            cpu_percent: self.sys.global_cpu_usage(),
            cpu_cores: self.sys.cpus().len(),
            load: [load.one, load.five, load.fifteen],
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            disks: self.collect_disks(),
            net_rx_bps: (rx as f64 / elapsed) as u64,
            net_tx_bps: (tx as f64 / elapsed) as u64,
            docker: self.docker.clone(),
            // Filled in by main.rs, which knows the agent's settings.
            can_control: false,
            can_reboot: false,
        }
    }

    fn collect_disks(&self) -> Vec<Disk> {
        let mut out: Vec<Disk> = Vec::new();
        for d in self.disks.list() {
            let mount = d.mount_point().to_string_lossy().to_string();
            if !wanted_mount(&mount) || d.total_space() == 0 || out.iter().any(|x| x.mount == mount)
            {
                continue;
            }
            out.push(Disk {
                used: d.total_space().saturating_sub(d.available_space()),
                total: d.total_space(),
                mount,
            });
        }
        out.sort_by(|a, b| a.mount.cmp(&b.mount));
        out
    }
}

/// Skip boot partitions, snaps and other noise; keep the mounts you care
/// about (like /mnt/storage on falc).
fn wanted_mount(m: &str) -> bool {
    m == "/"
        || m == "/home"
        || m.starts_with("/mnt/")
        || m.starts_with("/media/")
        || m.starts_with("/srv")
}

/// Count real interfaces only. Virtual ones (docker bridges, veth pairs,
/// tailscale) would double-count traffic that also crosses the real NIC.
fn is_physical_interface(name: &str) -> bool {
    !(name == "lo"
        || name.starts_with("docker")
        || name.starts_with("veth")
        || name.starts_with("br-")
        || name.starts_with("tailscale")
        || name.starts_with("virbr"))
}

fn collect_docker() -> Docker {
    let out = match exec::run(
        "docker",
        &[
            "ps",
            "-a",
            "--format",
            "{{.Names}}\t{{.Image}}\t{{.State}}\t{{.Status}}",
        ],
        DOCKER_TIMEOUT,
        1024 * 1024,
    ) {
        Ok(o) => o,
        Err(RunError::NotFound) => {
            return Docker {
                available: false,
                ..Default::default()
            };
        }
        Err(RunError::TimedOut) => return docker_error("docker timed out"),
        Err(RunError::Other(e)) => return docker_error(&e),
    };
    if !out.success {
        return docker_error(exec::first_line(&out.text));
    }

    // Lines that aren't 4 tab-separated fields (e.g. CLI warnings) are skipped.
    let stdout = &out.text;
    let mut containers: Vec<Container> = stdout
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (f.len() == 4).then(|| Container {
                name: f[0].into(),
                image: f[1].into(),
                state: f[2].into(),
                status: f[3].into(),
            })
        })
        .collect();

    // Running first, then alphabetical.
    containers.sort_by(|a, b| {
        (b.state == "running")
            .cmp(&(a.state == "running"))
            .then_with(|| a.name.cmp(&b.name))
    });

    Docker {
        available: true,
        error: None,
        containers,
    }
}

fn docker_error(msg: &str) -> Docker {
    Docker {
        available: true,
        error: Some(msg.trim().to_string()),
        containers: vec![],
    }
}
