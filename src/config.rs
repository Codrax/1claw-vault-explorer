// Remembers the non-secret bits of the login form between runs.
// The api key itself is never written to disk.

use crate::api::{DEFAULT_BASE_URL, KeyKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub base_url: String,
    pub kind: KeyKind,
    pub agent_id: String,
    pub dark: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            kind: KeyKind::User,
            agent_id: String::new(),
            dark: true,
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
            let _ = std::fs::write(p, s);
        }
    }
}
