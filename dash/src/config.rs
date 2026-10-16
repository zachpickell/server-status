//! Reads the list of servers from the config file.
//!
//! Location (first one that applies):
//!   $SERVER_STATUS_CONFIG
//!   $XDG_CONFIG_HOME/server-status/config.toml
//!   ~/.config/server-status/config.toml

use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize, Debug, Default)]
pub struct Config {
    /// Keyed by the name you type, e.g. `server-status falc`.
    /// A BTreeMap keeps them in alphabetical order for the overview.
    #[serde(default)]
    pub servers: BTreeMap<String, Server>,
    #[serde(default)]
    pub alert: Alert,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Server {
    /// "100.71.44.84:9100" or "http://host:9100". `/stats` is added automatically.
    pub url: String,
    pub token: Option<String>,
}

/// Each field is a separate alert channel. Add one (e.g. `email`) when you need it.
#[derive(Deserialize, Debug)]
pub struct Alert {
    pub discord: Option<Discord>,
    // pub email: Option<Email>, // TODO
    /// How often `server-status watch` checks the servers, in seconds.
    #[serde(default = "default_interval_secs")]
    pub interval_secs: u64,
}

impl Default for Alert {
    fn default() -> Self {
        Self {
            discord: None,
            interval_secs: default_interval_secs(),
        }
    }
}

fn default_interval_secs() -> u64 {
    60
}

#[derive(Deserialize, Debug, Clone)]
pub struct Discord {
    pub webhook: String,
    /// Discord role ID to @-mention on alerts, e.g. "123456789012345678".
    pub role_id: Option<String>,
}

pub const EXAMPLE: &str = r#"[servers.falc]
url = "100.x.y.z:9100"
token = "falc-token"

[servers.talon]
url = "100.a.b.c:9100"
token = "talon-token""#;

pub fn path() -> Option<PathBuf> {
    if let Some(p) = env::var_os("SERVER_STATUS_CONFIG").filter(|p| !p.is_empty()) {
        return Some(p.into());
    }
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .or_else(|| env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("server-status").join("config.toml"))
}

/// Load the config, or return a message explaining what's wrong.
pub fn load() -> Result<(Config, PathBuf), String> {
    let path = path().ok_or("couldn't work out your home folder (HOME/USERPROFILE is not set)")?;

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "no config file at {}\n\nCreate it with your servers, for example:\n\n{EXAMPLE}",
                path.display()
            ));
        }
        Err(e) => return Err(format!("couldn't read {}: {e}", path.display())),
    };

    let config = parse(&text).map_err(|e| format!("{} is not valid:\n{e}", path.display()))?;
    Ok((config, path))
}

pub fn parse(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_servers() {
        let c = parse(EXAMPLE).unwrap();
        let names: Vec<_> = c.servers.keys().cloned().collect();
        assert_eq!(names, ["falc", "talon"]);
        assert_eq!(c.servers["falc"].url, "100.x.y.z:9100");
        assert_eq!(c.servers["talon"].token.as_deref(), Some("talon-token"));
    }

    #[test]
    fn token_is_optional() {
        let c = parse("[servers.box]\nurl = \"box:9100\"\n").unwrap();
        assert!(c.servers["box"].token.is_none());
    }

    #[test]
    fn interval_secs_defaults_to_60_but_can_be_set() {
        assert_eq!(parse(EXAMPLE).unwrap().alert.interval_secs, 60);
        let c = parse("[alert]\ninterval_secs = 30\n").unwrap();
        assert_eq!(c.alert.interval_secs, 30);
    }

    #[test]
    fn missing_url_is_an_error() {
        assert!(parse("[servers.box]\ntoken = \"x\"\n").is_err());
    }
}
