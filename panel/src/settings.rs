//! Panel settings (read-only, from panel.toml) and the server list
//! (read-write, edited from the web page, saved to servers.toml).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// /etc/server-panel/panel.toml
#[derive(Deserialize, Debug)]
pub struct PanelConfig {
    /// Where to serve the web page, e.g. "100.91.77.102:8080".
    #[serde(default = "default_listen")]
    pub listen: String,
    /// Output of `server-panel hash-password`.
    pub password_hash: String,
    /// Where the server list is saved. Defaults to $STATE_DIRECTORY/servers.toml
    /// (set by systemd), or servers.toml next to the config file.
    pub servers_file: Option<PathBuf>,
}

fn default_listen() -> String {
    "127.0.0.1:8080".into()
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ServerList {
    #[serde(default)]
    pub servers: BTreeMap<String, ServerEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerEntry {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

pub fn load_config(path: &Path) -> Result<PanelConfig, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let cfg: PanelConfig =
        toml::from_str(&text).map_err(|e| format!("{} is not valid:\n{e}", path.display()))?;
    if !cfg.password_hash.starts_with("$argon2") {
        return Err(format!(
            "password_hash in {} doesn't look right. Generate one with: server-panel hash-password",
            path.display()
        ));
    }
    Ok(cfg)
}

pub fn servers_file(cfg: &PanelConfig, config_path: &Path) -> PathBuf {
    if let Some(p) = &cfg.servers_file {
        return p.clone();
    }
    if let Some(dir) = std::env::var_os("STATE_DIRECTORY").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir).join("servers.toml");
    }
    config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("servers.toml")
}

pub fn load_servers(path: &Path) -> Result<ServerList, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            toml::from_str(&text).map_err(|e| format!("{} is not valid:\n{e}", path.display()))
        }
        // First run: start with an empty list.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ServerList::default()),
        Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
    }
}

/// Write to a temp file then rename, so a crash mid-save can't leave a
/// half-written file. The file holds agent tokens, so it's owner-only.
pub fn save_servers(path: &Path, list: &ServerList) -> Result<(), String> {
    let text = toml::to_string_pretty(list).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("couldn't write {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("couldn't save {}: {e}", path.display()))
}

/// Server names show up in URLs and commands: keep them simple.
pub fn valid_server_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// "100.1.2.3:9100", "http://host:9100/", "host:9100/stats" -> "http://host:9100"
pub fn normalize_agent_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() || url.chars().any(char::is_whitespace) {
        return Err("enter an address like 100.71.44.84:9100".into());
    }
    let mut url = url.trim_end_matches('/').to_string();
    if let Some(stripped) = url.strip_suffix("/stats") {
        url = stripped.to_string();
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("http://{url}");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_server_name("falc"));
        assert!(valid_server_name("talon-2"));
        assert!(!valid_server_name(""));
        assert!(!valid_server_name("a b"));
        assert!(!valid_server_name("../etc"));
    }

    #[test]
    fn urls() {
        assert_eq!(
            normalize_agent_url("100.1.2.3:9100").unwrap(),
            "http://100.1.2.3:9100"
        );
        assert_eq!(
            normalize_agent_url("http://h:1/stats/").unwrap(),
            "http://h:1"
        );
        assert!(normalize_agent_url("  ").is_err());
        assert!(normalize_agent_url("a b:1").is_err());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("panel-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("servers.toml");
        let mut list = ServerList::default();
        list.servers.insert(
            "falc".into(),
            ServerEntry {
                url: "http://a:1".into(),
                token: Some("t".into()),
            },
        );
        save_servers(&path, &list).unwrap();
        let back = load_servers(&path).unwrap();
        assert_eq!(back.servers["falc"].token.as_deref(), Some("t"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
