# server-dashboard

Watch and manage your servers from the terminal or a web page, written in Rust.

It's three programs:

- **`server-agent`** runs on each server. It samples CPU, memory, disks, network, uptime and Docker containers once per second, serves them as JSON, and accepts a few actions: start/stop/restart a container, read its logs, and (if you allow it) reboot.
- **`server-status`** runs on your laptop or desktop. Give it a server name for a live terminal dashboard, or no name for a quick summary of everything.
- **`server-panel`** is a web page that runs on one always-on machine (talon). It shows every server and lets you restart containers, read logs, reboot servers, and add or edit servers from a Settings page.

```
$ server-status
SERVER  STATUS     CPU    MEM  DISK (fullest)        UPTIME       DOCKER
falc    ● up     12.5%    62%   92% /mnt/storage     3d 4h 12m    5/6 running
talon   ● DOWN  connection refused
```

## How it works

```
 MacBook ─ server-status ──┐
                           │                        ┌──► falc  (server-agent)
 Desktop ─ server-status ──┼── Tailscale, HTTP ─────┤
                           │   + per-server token   └──► talon (server-agent)
 Browser ─► server-panel ──┘
            (on talon)
```

- Each agent keeps its latest snapshot in memory, so requests are instant. It uses about 3 MB of RAM.
- Any number of `server-status` copies and the panel can talk to the same agents at once.
- Every request to an agent needs `Authorization: Bearer <token>`. Container actions are only enabled when the agent has a token, and reboot also needs `AGENT_ALLOW_REBOOT=true` plus a polkit rule.
- The panel keeps agent tokens on talon. The browser only ever talks to the panel, which is password-protected.

## Project layout

```
common/   shared types (the JSON format)
agent/    server-agent: stats, container actions, logs, reboot
dash/     server-status: terminal summary + live dashboard
panel/    server-panel: web UI (panel/static/index.html is the whole page)
deploy/   systemd units, polkit rules, example configs
```

## Server setup (every server)

Do this on falc, talon, and any server you add later. It needs Tailscale, Rust (`curl https://sh.rustup.rs -sSf | sh`) and the repo cloned.

```bash
cd ~/server-dashboard && git pull
cargo build --release -p server-agent

# 1. A system user for the agent (skip if it already exists)
sudo useradd --system --home-dir /var/lib/server-agent --no-create-home --shell /usr/sbin/nologin server-agent
# Let it use Docker (only if this server has Docker)
getent group docker && sudo usermod -aG docker server-agent

# 2. Install
sudo systemctl stop server-agent 2>/dev/null
sudo cp target/release/server-agent /usr/local/bin/
sudo cp deploy/server-agent.service /etc/systemd/system/

# 3. Config (first time only)
sudo mkdir -p /etc/server-agent
sudo cp -n deploy/agent.env.example /etc/server-agent/agent.env
sudo nano /etc/server-agent/agent.env      # AGENT_LISTEN, AGENT_TOKEN, AGENT_ALLOW_REBOOT
sudo chmod 600 /etc/server-agent/agent.env

# 4. Start
sudo systemctl daemon-reload
sudo systemctl enable --now server-agent
```

### Allowing reboot (optional, per server)

1. Set `AGENT_ALLOW_REBOOT=true` in `/etc/server-agent/agent.env`.
2. Install the polkit rule that lets the `server-agent` user reboot, and nothing else. Check your polkit version with `pkaction --version`:
   ```bash
   # polkit 121 or newer (Ubuntu 23.10+, including 24.04):
   sudo cp deploy/50-server-agent-reboot.rules /etc/polkit-1/rules.d/
   # older polkit (0.105, Ubuntu 22.04):
   sudo cp deploy/server-agent-reboot.pkla /etc/polkit-1/localauthority/50-local.d/
   ```
3. Run `sudo systemctl restart server-agent`.

## Web panel setup (talon)

```bash
cd ~/server-dashboard && git pull
cargo build --release -p server-panel

sudo useradd --system --no-create-home --shell /usr/sbin/nologin server-panel
sudo cp target/release/server-panel /usr/local/bin/
sudo cp deploy/server-panel.service /etc/systemd/system/

sudo mkdir -p /etc/server-panel
sudo cp deploy/panel.toml.example /etc/server-panel/panel.toml
server-panel hash-password                 # prints a password_hash line
sudo nano /etc/server-panel/panel.toml     # set listen (talon's Tailscale IP:8080) and password_hash
sudo chown root:server-panel /etc/server-panel/panel.toml
sudo chmod 640 /etc/server-panel/panel.toml

sudo systemctl daemon-reload
sudo systemctl enable --now server-panel
```

Open `http://<talon's Tailscale IP>:8080`, sign in, go to **Settings**, and add each server with its agent address and token. If talon's firewall is on, allow the port on Tailscale only:

```bash
sudo ufw allow in on tailscale0 to any port 8080 proto tcp
```

If the build is killed on a small VPS, it ran out of memory. Build on falc instead and copy the binary over with `scp target/release/server-panel talon:~`.

## Terminal setup (your computers)

```bash
git clone https://github.com/zachpickell/server-dashboard.git && cd server-dashboard
cargo install --path dash      # installs server-status into ~/.cargo/bin
```

Create `~/.config/server-status/config.toml`, then `chmod 600` it:

```toml
[servers.falc]
url = "100.x.y.z:9100"
token = "falc-token"

[servers.talon]
url = "100.a.b.c:9100"
token = "talon-token"
```

| Command | What it does |
|---|---|
| `server-status` | One-line summary of every server. Exit code 1 if any are down. |
| `server-status falc` | Live dashboard for falc (`-falc` also works) |
| `server-status 100.x.y.z:9100` | Dashboard for an address not in the config (token from `DASH_TOKEN`) |

Dashboard keys: `q` quit, `r` refresh now.

## Security notes

- **Everything stays on Tailscale.** Agents and the panel should listen on Tailscale IPs, never on public addresses.
- **Panel login.** The password is stored as an Argon2 hash. Sessions are random 256-bit IDs in an HttpOnly, SameSite=Strict cookie, and last 7 days or until the panel restarts. Every state-changing request needs an `X-Panel` header, which blocks cross-site request forgery. 10 wrong passwords in 10 minutes locks login for a while.
- **Agent tokens.** Each server should have its own token. The panel saves them in `/var/lib/server-panel/servers.toml` (mode 600) and never sends them to the browser.
- **Docker group.** The `server-agent` user is in the `docker` group, which is effectively root on that host. The agent only ever runs `docker ps`, `start`, `stop`, `restart` and `logs`, and container names are validated before use.
- **Reboot** is off unless you turn it on per server, and the panel makes you type the server's name to confirm.

## Development

```bash
cargo test
cargo clippy --all-targets
AGENT_LISTEN=127.0.0.1:9100 AGENT_TOKEN=dev cargo run -p server-agent
cargo run -p server-panel -- --config ./panel.toml
cargo run -p server-status -- 127.0.0.1:9100
```

## Roadmap

- [x] Monitor multiple servers
- [x] Web panel: container restart/start/stop, logs, reboot, server settings
- [ ] Alerts when a server goes down (Discord webhook from the panel)
- [ ] Per-container CPU and memory

## AI disclosure

Built with AI assistance (Claude) as a learning project.
