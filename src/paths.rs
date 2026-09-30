//! Well-known locations. Every path honours the XDG overrides so the whole app
//! can be pointed at a scratch copy of `~/.config` for testing.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".config"))
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".cache"))
        .join("nexus-tasks")
}

pub fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".local/state"))
}

/// `~/.config/nexus-tasks`: everything this app owns lives here.
pub fn app_dir() -> PathBuf {
    config_home().join("nexus-tasks")
}

pub fn prefs_file() -> PathBuf {
    app_dir().join("settings.toml")
}

pub fn state_file() -> PathBuf {
    app_dir().join("state.json")
}

pub fn custom_themes_dir() -> PathBuf {
    app_dir().join("themes")
}

pub fn hypr_dir() -> PathBuf {
    config_home().join("hypr")
}

/// The single Hyprland file this app writes (startup items it manages).
pub fn managed_lua() -> PathBuf {
    hypr_dir().join("tasks.lua")
}

pub fn hyprland_lua() -> PathBuf {
    hypr_dir().join("hyprland.lua")
}

pub fn user_autostart_lua() -> PathBuf {
    hypr_dir().join("autostart.lua")
}

pub fn omarchy_autostart_lua() -> PathBuf {
    let root = std::env::var_os("OMARCHY_PATH").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"));
    root.join("default/hypr/autostart.lua")
}

pub fn user_xdg_autostart() -> PathBuf {
    config_home().join("autostart")
}

pub fn system_xdg_autostart() -> PathBuf {
    PathBuf::from("/etc/xdg/autostart")
}

pub fn omarchy_theme_dir() -> PathBuf {
    state_home().join("omarchy/current/theme")
}

pub fn omarchy_colors() -> PathBuf {
    omarchy_theme_dir().join("colors.toml")
}

/// Replace `$HOME` with `~` for display.
pub fn pretty(path: &std::path::Path) -> String {
    let home = home();
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}
