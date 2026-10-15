# server-status

Check on your servers from any of your computers, written in Rust.

It's two programs:

- **`server-agent`** runs on each server you want to watch. It samples CPU, memory, disks, network, uptime and Docker containers once per second and serves them as JSON over HTTP.
- **`server-status`** runs on your laptop or desktop. Give it a server name for a live dashboard, or no name for a quick summary of everything.

```
$ server-status
SERVER  STATUS     CPU    MEM  DISK (fullest)        UPTIME       DOCKER
falc    ● up     12.5%    62%   92% /mnt/storage     3d 4h 12m    5/6 running
talon   ● DOWN  connection refused

$ server-status falc
 falc  Linux (Ubuntu 24.04) · up 3d 4h 12m                                         ● UP
╭ CPU ─────────────────────────────────╮╭ Memory ──────────────────────────────╮
│█████████████37.5%                    ││RAM 10.0 GiB / 16.0 GiB               │
│      ▂▃▅▇█████     ▁▃▄▆█████    ▁▂▄▆ ││█████████████████62.5%██              │
│8 cores · load 1.20 0.90 0.70         ││Swap 0 B / 4.0 GiB                    │
╰──────────────────────────────────────╯╰──────────────────────────────────────╯
╭ Disks ───────────────────────────────╮╭ Network ─────────────────────────────╮
│/             ██ 26%  120.0 GiB / 460 ││↓ in  1.4 MiB/s                       │
│/mnt/storage  ████92%   3.3 TiB / 3.6 ││↑ out 244.1 KiB/s                     │
╰──────────────────────────────────────╯╰──────────────────────────────────────╯
╭ Docker · 1/2 running ────────────────────────────────────────────────────────╮
│NAME                  IMAGE                         STATUS                    │
│● inbox-zero          ghcr.io/elie222/inbox-zero    Up 3 hours                │
│● old-thing           nginx                         Exited (0) 2 days ago     │
╰──────────────────────────────────────────────────────────────────────────────╯
```

## How it works

```
 MacBook ──┐                         ┌──► falc  (server-agent)
           ├── Tailscale, HTTP ──────┤
 Desktop ──┘   GET /stats + token    └──► talon (server-agent)
```

- Each agent keeps its latest snapshot in memory, so requests are instant. It uses about 3 MB of RAM.
- Any number of machines can run `server-status` against the same agents at once.
- The dashboard does network calls on a background thread, so a dead server never freezes the UI.
- 3 failed polls in a row (about 6 seconds) and the dashboard shows **DOWN**, with the last known data greyed out.
- Requests need `Authorization: Bearer <token>` when the agent has `AGENT_TOKEN` set.

## Project layout

```
common/   shared types (the JSON format)
agent/    server-agent: collects stats, serves /stats and /health
dash/     server-status: summary + live dashboard
deploy/   systemd unit and example config for the agent
```

## Setup

Every machine needs to be on the same Tailscale network.

### 1. On each server: the agent

```bash
sudo apt update && sudo apt install -y build-essential git curl gh
curl https://sh.rustup.rs -sSf | sh && source ~/.cargo/env
gh auth login && gh repo clone zachpickell/server-status && cd server-status

cargo build --release -p server-agent
sudo cp target/release/server-agent /usr/local/bin/
sudo cp deploy/server-agent.service /etc/systemd/system/
sudo mkdir -p /etc/server-agent
sudo cp deploy/agent.env.example /etc/server-agent/agent.env
sudo nano /etc/server-agent/agent.env      # AGENT_LISTEN=<tailscale ip>:9100, AGENT_TOKEN=<openssl rand -hex 32>
sudo chmod 600 /etc/server-agent/agent.env
sudo systemctl daemon-reload && sudo systemctl enable --now server-agent
```

If the server doesn't have Docker (`getent group docker` prints nothing), remove the Docker line from the service file first, or the agent won't start:

```bash
sudo sed -i '/SupplementaryGroups=docker/d' /etc/systemd/system/server-agent.service
```

Check it works:

```bash
curl -H "Authorization: Bearer YOUR_TOKEN" http://$(tailscale ip -4):9100/stats
```

### 2. On your computers: `server-status`

```bash
git clone https://github.com/zachpickell/server-status.git && cd server-status
cargo install --path dash      # installs server-status into ~/.cargo/bin
```

Then create `~/.config/server-status/config.toml`:

```toml
[servers.falc]
url = "100.x.y.z:9100"
token = "falc-token"

[servers.talon]
url = "100.a.b.c:9100"
token = "talon-token"
```

```bash
chmod 600 ~/.config/server-status/config.toml
```

Don't commit this file anywhere public, because it holds your tokens.

## Usage

| Command | What it does |
|---|---|
| `server-status` | One-line summary of every server, then exits. Exit code 1 if any are down. |
| `server-status falc` | Live dashboard for falc (`-falc` also works) |
| `server-status 100.x.y.z:9100` | Dashboard for an address not in the config (token from `DASH_TOKEN`) |
| `server-status --help` | Help |

Dashboard keys: `q` quit, `r` refresh now.

Set `SERVER_STATUS_CONFIG` to use a config file somewhere else.

## Agent config

| Variable | Default | Meaning |
|---|---|---|
| `AGENT_LISTEN` | `0.0.0.0:9100` | Address to listen on (use the Tailscale IP) |
| `AGENT_TOKEN` | none | Required bearer token |

## Notes

- **Docker access.** The agent runs `docker ps`, so its user is in the `docker` group, which is effectively root on that host. The agent only ever runs `docker ps`, but keep the port on Tailscale and keep the token set.
- **Which disks show up.** `/`, `/home`, and anything under `/mnt`, `/media` or `/srv`.
- **Updating.** Run `git pull`, then `cargo install --path dash` on your computers. On servers, rebuild the agent, `sudo systemctl stop server-agent`, copy the binary, then `start` it again.

## Development

```bash
cargo test
cargo clippy
AGENT_LISTEN=127.0.0.1:9100 cargo run -p server-agent
cargo run -p server-status -- 127.0.0.1:9100
```

## Roadmap

- [x] Monitor multiple servers
- [ ] Alerts when a server goes down (Discord webhook, running on an always-on machine)
- [ ] Per-container CPU and memory
- [ ] Restart containers from the dashboard

## AI disclosure

Built with AI assistance (Claude) as a learning project.
