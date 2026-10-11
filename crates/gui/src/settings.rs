use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const BITRATES: [u32; 4] = [320, 256, 192, 128];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub dark: bool,
    pub bitrate_kbps: u32,
    pub out_dir: PathBuf,
}

impl Default for Settings {
    fn default() -> Self {
        let music = directories::UserDirs::new()
            .and_then(|u| u.audio_dir().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            dark: true,
            bitrate_kbps: 320,
            out_dir: music.join("spool"),
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "spool")
            .map(|d| d.config_dir().join("settings.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Best effort: a settings file that cannot be written is not worth
    /// interrupting anyone for.
    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn next_bitrate(&mut self) {
        let i = BITRATES
            .iter()
            .position(|b| *b == self.bitrate_kbps)
            .unwrap_or(0);
        self.bitrate_kbps = BITRATES[(i + 1) % BITRATES.len()];
    }
}
