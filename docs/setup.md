# Setup

Every machine needs to be on the same Tailscale network.

## 1. On each server: the agent

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

## 2. On your computers: `server-status`

```bash
git clone https://github.com/zachpickell/server-status.git && cd server-status
cargo install --path dash      # installs server-status into ~/.cargo/bin
```

Then create `~/.config/server-status/config.toml` — see [config.md](config.md) for the full format (servers, alerts):

```toml
[servers.web]
url = "100.x.y.z:9100"
token = "web-token"

[servers.db]
url = "100.a.b.c:9100"
token = "db-token"
```

```bash
chmod 600 ~/.config/server-status/config.toml
```

Don't commit this file anywhere public, because it holds your tokens.
