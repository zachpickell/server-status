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
}

#[derive(Deserialize, Debug, Clone)]
pub struct Server {
    /// "100.71.44.84:9100" or "http://host:9100". `/stats` is added automatically.
    pub url: String,
    pub token: Option<String>,
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
    fn missing_url_is_an_error() {
        assert!(parse("[servers.box]\ntoken = \"x\"\n").is_err());
    }
}
