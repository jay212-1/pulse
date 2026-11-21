//! Per-computer settings: recently opened profiles and the last folder.
//! Stored in `<config dir>/pulse/settings.json`; `PULSE_CONFIG_DIR` points
//! somewhere else (tests never touch the real settings).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Recent profiles listed on the start page.
pub const MAX_RECENT: usize = 10;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub recent: Vec<PathBuf>,
    pub last_dir: Option<PathBuf>,
    /// Alert sound in show mode (off by default).
    pub sound: bool,
}

/// The settings folder.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("PULSE_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    Some(dirs::config_dir()?.join("pulse"))
}

/// Where new profiles go unless another place is chosen: Documents/Pulse.
pub fn default_profile_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
        .map(|d| d.join("Pulse"))
        .unwrap_or_default()
}

impl Settings {
    pub fn load() -> Settings {
        config_dir()
            .map(|d| d.join("settings.json"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(dir) = config_dir() else { return };
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = pulse_core::profile::atomic_write(&dir.join("settings.json"), text.as_bytes());
        }
    }

    /// Put `path` first in the recent list.
    pub fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
        self.last_dir = path.parent().map(Path::to_path_buf);
        self.save();
    }
}
