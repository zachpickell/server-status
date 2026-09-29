# server-dashboard

A terminal dashboard for monitoring a home server from a separate machine, written in Rust.

It's two small programs:

- **`server-agent`** runs on the server being monitored. It samples CPU, memory, disks, network, uptime and Docker containers once per second and serves them as JSON over HTTP.
- **`server-dash`** runs on a different machine (a VPS). It polls the agent and draws a live dashboard in the terminal. If the agent stops answering, the dashboard switches to **DOWN** and shows how long it's been unreachable.

Running the dashboard off the server is the point: if the server dies, the thing watching it doesn't die with it.

```
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
   falc (home server)                         VPS
 ┌─────────────────────┐   HTTP over     ┌──────────────────┐
 │ server-agent        │   Tailscale     │ server-dash      │
 │  sampler thread ────┼──► GET /stats ◄─┼── poller thread  │
 │  (sysinfo, docker)  │   JSON + token  │  Ratatui UI      │
 └─────────────────────┘                 └──────────────────┘
```

- The agent keeps the latest snapshot in memory, so requests are instant. It uses about 3 MB of RAM.
- The dashboard does network calls on a background thread, so a dead server never freezes the UI.
- 3 failed polls in a row (about 6 seconds) and the status goes to **DOWN**. The last known data stays on screen, greyed out.
- Requests need `Authorization: Bearer <token>` when `AGENT_TOKEN` is set.

## Project layout

```
common/   shared types (the JSON format)
agent/    server-agent: collects stats, serves /stats and /health
dash/     server-dash: polls the agent, draws the TUI
deploy/   systemd unit and example config for the agent
```

## Setup

Both machines need Rust (`curl https://sh.rustup.rs -sSf | sh`) and both should be on the same Tailscale network.

### 1. On falc: the agent

```bash
git clone <this repo> && cd server-dashboard
cargo build --release -p server-agent

sudo cp target/release/server-agent /usr/local/bin/
sudo cp deploy/server-agent.service /etc/systemd/system/
sudo mkdir -p /etc/server-agent
sudo cp deploy/agent.env.example /etc/server-agent/agent.env
sudo nano /etc/server-agent/agent.env     # set AGENT_LISTEN to falc's Tailscale IP, set a token
sudo chmod 600 /etc/server-agent/agent.env
sudo systemctl daemon-reload
sudo systemctl enable --now server-agent
```

Check it's running:

```bash
curl -H "Authorization: Bearer YOUR_TOKEN" http://$(tailscale ip -4):9100/stats
```

### 2. On the VPS: the dashboard

```bash
git clone <this repo> && cd server-dashboard
cargo build --release -p server-dash
sudo cp target/release/server-dash /usr/local/bin/

export DASH_TOKEN=YOUR_TOKEN
server-dash falc:9100          # or falc's Tailscale IP
```

Put `export DASH_TOKEN=...` in `~/.bashrc` so you don't have to type it every time.

## Config

| Program | Variable | Default | Meaning |
|---|---|---|---|
| agent | `AGENT_LISTEN` | `0.0.0.0:9100` | Address to listen on |
| agent | `AGENT_TOKEN` | none | Required bearer token |
| dash | first argument or `DASH_URL` | `http://127.0.0.1:9100` | Agent address (`/stats` is added automatically) |
| dash | `DASH_TOKEN` | none | Must match `AGENT_TOKEN` |

Keys: `q` quit, `r` refresh now.

## Notes

- **Docker access.** The agent runs `docker ps`, so its user needs to be in the `docker` group. That group is effectively root on the host. The agent only ever runs `docker ps`, but keep the port on Tailscale and keep the token set.
- **Which disks show up.** `/`, `/home`, and anything under `/mnt`, `/media` or `/srv`. Boot partitions and snaps are hidden.

## Development

```bash
cargo test        # includes rendering the UI into a fake terminal
cargo clippy
AGENT_LISTEN=127.0.0.1:9100 cargo run -p server-agent
cargo run -p server-dash -- 127.0.0.1:9100
```

## Roadmap

- [ ] Alerts when the server goes down (Discord webhook)
- [ ] Monitor multiple servers
- [ ] Per-container CPU and memory
- [ ] Restart containers from the dashboard

## AI disclosure

Built with AI assistance (Claude) as a learning project.
