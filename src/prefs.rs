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
    /// The sidebar shows only icons, whatever the window width.
    pub sidebar_collapsed: bool,
    pub glow: bool,
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
    /// Processes column widths in pixels, by column id.
    pub column_widths: std::collections::HashMap<String, i32>,
    /// Processes column order, by column id.
    pub column_order: Vec<String>,
    /// Processes sort column id and direction.
    pub sort_column: String,
    pub sort_desc: bool,
    /// Overview tiles to show, in order: cpu, mem, gpu, disk, net, temp, bat.
    pub overview_tiles: Vec<String>,
    /// The metric each "Busiest processes" card ranks by: cpu, mem, disk or gpu.
    pub overview_top: Vec<String>,
    /// The CPU page shows each thread as a small tile instead of a graph.
    pub cpu_grid: bool,
    /// Warning thresholds: CPU temperature (°C), memory use and disk fullness (%).
    pub warn_temp: f64,
    pub warn_mem: f64,
    pub warn_disk: f64,
    /// Desktop notifications: CPU temperature and memory past their warning
    /// thresholds, a process over `alert_proc_cpu`% for `alert_proc_secs`, a failed service.
    pub alert_temp: bool,
    pub alert_mem: bool,
    pub alert_proc: bool,
    pub alert_proc_cpu: f64,
    pub alert_proc_secs: u64,
    pub alert_services: bool,
    /// The process details panel shows only its head line.
    pub details_compact: bool,
    /// Services page: "user" or "system".
    pub services_scope: String,
    /// Services page: list timers instead of services.
    pub services_timers: bool,
    /// Services page: all, running or failed.
    pub services_show: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Omarchy,
            theme: "tokyo-night".into(),
            reduce_motion: false,
            sidebar_collapsed: false,
            glow: true,
            interval_ms: 1000,
            history_secs: 60,
            net_bits: false,
            fahrenheit: false,
            confirm_kill: true,
            kernel_threads: false,
            pause_hidden: true,
            hidden_columns: vec!["threads".into(), "state".into()],
            process_view: "all".into(),
            column_widths: Default::default(),
            column_order: Vec::new(),
            sort_column: "cpu".into(),
            sort_desc: true,
            overview_tiles: ["cpu", "mem", "gpu", "disk", "net", "temp", "bat"].map(String::from).to_vec(),
            overview_top: vec!["cpu".into(), "mem".into()],
            cpu_grid: false,
            warn_temp: 85.0,
            warn_mem: 90.0,
            warn_disk: 95.0,
            alert_temp: false,
            alert_mem: false,
            alert_proc: false,
            alert_proc_cpu: 50.0,
            alert_proc_secs: 30,
            alert_services: false,
            details_compact: false,
            services_scope: "user".into(),
            services_timers: false,
            services_show: "all".into(),
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
