// Remembers the login form + theme between runs.
// The api key is only stored when "remember me" is ticked, and the file is
// locked down to the current user (0600) on unix.

use crate::api::{DEFAULT_BASE_URL, KeyKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)] // so older config files without the newer fields still load
pub struct Config {
    pub base_url: String,
    pub kind: KeyKind,
    pub agent_id: String,
    pub dark: bool,
    pub remember: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub api_key: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            kind: KeyKind::User,
            agent_id: String::new(),
            dark: true,
            remember: false,
            api_key: String::new(),
        }
    }
}

fn file() -> Option<PathBuf> {
    directories::ProjectDirs::from("co", "1claw", "vault-explorer").map(|d| d.config_dir().join("config.json"))
}

impl Config {
    pub fn load() -> Self {
        file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = file() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = write_private(&p, s.as_bytes());
        }
    }
}

#[cfg(unix)]
fn write_private(p: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(p)?;
    // mode() only applies on create, tighten files left over from older versions too
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.write_all(data)
}

#[cfg(not(unix))]
fn write_private(p: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    std::fs::write(p, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_still_loads() {
        let old = r#"{"base_url":"http://localhost:8443","kind":"Agent","agent_id":"","dark":false}"#;
        let c: Config = serde_json::from_str(old).unwrap();
        assert!(!c.remember && c.api_key.is_empty() && !c.dark);
    }

    #[test]
    fn key_not_written_when_empty() {
        let s = serde_json::to_string(&Config::default()).unwrap();
        assert!(!s.contains("api_key"));
    }
}
