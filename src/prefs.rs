//! This app's own preferences (`~/.config/nexus-tasks/settings.toml`).

use crate::{cmd, paths};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    /// Follow the active Omarchy theme live.
    Omarchy,
    /// Use a bundled or custom theme.
    Theme,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub mode: ThemeMode,
    pub theme: String,
    pub reduce_motion: bool,
    pub glow: bool,
    pub last_section: String,
    /// How often to sample, in milliseconds.
    pub interval_ms: u64,
    /// How much history the graphs keep, in seconds.
    pub history_secs: u64,
    /// Show network rates in bits per second instead of bytes.
    pub net_bits: bool,
    /// Show temperatures in °F instead of °C.
    pub fahrenheit: bool,
    /// Ask before ending a task.
    pub confirm_kill: bool,
    /// List kernel threads in Processes.
    pub kernel_threads: bool,
    /// Stop collecting per-process and GPU data while the window is hidden.
    pub pause_hidden: bool,
    /// Columns hidden in Processes.
    pub hidden_columns: Vec<String>,
    /// Processes view: apps, all, tree or mine.
    pub process_view: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Omarchy,
            theme: "tokyo-night".into(),
            reduce_motion: false,
            glow: true,
            last_section: "overview".into(),
            interval_ms: 1000,
            history_secs: 60,
            net_bits: false,
            fahrenheit: false,
            confirm_kill: true,
            kernel_threads: false,
            pause_hidden: true,
            hidden_columns: vec!["threads".into(), "state".into()],
            process_view: "all".into(),
        }
    }
}

thread_local! {
    static PREFS: RefCell<Prefs> = RefCell::new(load());
}

fn load() -> Prefs {
    std::fs::read_to_string(paths::prefs_file()).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default()
}

pub fn get() -> Prefs {
    PREFS.with(|p| p.borrow().clone())
}

pub fn update(change: impl FnOnce(&mut Prefs)) {
    PREFS.with(|p| {
        let mut prefs = p.borrow_mut();
        change(&mut prefs);
        if let Ok(text) = toml::to_string_pretty(&*prefs) {
            let _ = cmd::atomic_write(&paths::prefs_file(), &text);
        }
    });
}
