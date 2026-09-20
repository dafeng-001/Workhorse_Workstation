use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetState {
    #[serde(default = "def_true")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: String, // "slim" | "full"
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub monitor: Option<String>,
}

fn def_true() -> bool {
    true
}

impl Default for WidgetState {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "slim".into(),
            x: None,
            y: None,
            monitor: None,
        }
    }
}

fn path() -> PathBuf {
    crate::config::app_root().join("data").join("widget.json")
}

pub fn load() -> WidgetState {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(st: &WidgetState) {
    if let Some(p) = path().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(path(), serde_json::to_string_pretty(st).unwrap_or_default());
}

/// Logical sizes — must match frontend constants
pub const SLIM_W: f64 = 320.0;
pub const SLIM_H: f64 = 78.0;
pub const FULL_W: f64 = 320.0;
pub const FULL_H: f64 = 220.0;

pub fn size_for(mode: &str) -> (f64, f64) {
    if mode == "full" {
        (FULL_W, FULL_H)
    } else {
        (SLIM_W, SLIM_H)
    }
}
