# server-status

Check on your servers from any of your computers, written in Rust.

It's two programs:

- **`server-agent`** runs on each server you want to watch. It samples CPU, memory, disks, network, uptime and Docker containers once per second and serves them as JSON over HTTP.
- **`server-status`** runs on your laptop or desktop. Give it a server name for a live dashboard, or no name for a quick summary of everything.

```
$ server-status
SERVER  STATUS     CPU    MEM  DISK (fullest)        UPTIME       DOCKER
web     ● up     12.5%    62%   92% /mnt/storage     3d 4h 12m    5/6 running
db      ● DOWN  connection refused

$ server-status web
 web  Linux (Ubuntu 24.04) · up 3d 4h 12m                                          ● UP
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
 Laptop  ──┐                         ┌──► web  (server-agent)
           ├── Tailscale, HTTP ──────┤
 Desktop ──┘   GET /stats + token    └──► db   (server-agent)
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

See [docs/setup.md](docs/setup.md): install the agent on each server, install `server-status` on your computers, and write the config file. Every machine needs to be on the same Tailscale network.

## Usage

| Command | What it does |
|---|---|
| `server-status` | One-line summary of every server, then exits. Exit code 1 if any are down. |
| `server-status web` | Live dashboard for web (`-web` also works) |
| `server-status 100.x.y.z:9100` | Dashboard for an address not in the config (token from `DASH_TOKEN`) |
| `server-status watch` | Runs forever, checking every server and alerting on change (see Alerts below) |
| `server-status --help` | Help |

Dashboard keys: `q` quit, `r` refresh now.

Set `SERVER_STATUS_CONFIG` to use a config file somewhere else.

## Alerts

`server-status` can send a Discord message when a server goes down (and another when it comes back up). See [docs/config.md](docs/config.md) for the `[alert]` config options.

`server-status` (the one-shot summary) and `server-status web` (the live dashboard) alert too, but only for whatever they see on that one check — they don't keep watching after they exit. For an always-on check, run `server-status watch` in the background (e.g. as a systemd service, the same way the agent runs). It re-checks on the configured interval and only alerts on a change, not on every pass while a server stays down.

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

## AI disclosure

- Built with AI assistance (Claude) as a learning project.
- If you see something that could be done better please create an issue and open a PR.