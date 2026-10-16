## Configuration

The configuration file uses TOML. It defines the servers to monitor and
optional alert destinations. Each alert type has its own table and options;
configure only the alert types you want to use.

### File location

The application checks these locations in order:

1. The path in `SERVER_STATUS_CONFIG`, if set.
2. `$XDG_CONFIG_HOME/server-status/config.toml`, if `XDG_CONFIG_HOME` is set.
3. `~/.config/server-status/config.toml` (under the user profile on Windows).

### Servers

Add one `[servers.<name>]` table for each server. The table name is the name
used to identify the server in the application.

| Option | Required | Description |
| --- | --- | --- |
| `url` | Yes | Server address, such as `100.xx.xx.xx:9100` or `http://host:9100`. The application adds `/stats` automatically. |
| `token` | No | Authentication token for the server. |

```toml
[servers.falc]
url = "100.xx.xx.xx:9100"
token = "falc-token"

[servers.talon]
url = "http://host:9100"
```

### Alerts

Alert settings are nested under `[alert]`. Alert types are independent: each
type has its own table and may have different required and optional settings.
Omit an alert table to leave that alert type disabled.

| Option | Required | Description |
| --- | --- | --- |
| `interval_secs` | No | How often `server-status watch` checks the servers, in seconds. Defaults to `60`. |

```toml
[alert]
interval_secs = 60
```

#### Discord

Configure Discord notifications with `[alert.discord]`.

| Option | Required | Description |
| --- | --- | --- |
| `webhook` | Yes | Discord webhook URL to which alerts are sent. Treat it as a secret. |

```toml
[alert.discord]
webhook = "https://discord.com/api/webhooks/…"
```

Other alert types can be added as their own tables under `[alert]`; consult
their option lists when they are supported. Keep each type's settings in its
own table so options for one destination are not confused with another's.
