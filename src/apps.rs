//! Installed applications (`.desktop` files): names and icons for processes and
//! windows, and the list the "Add startup item" dialog picks from.

use crate::paths;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, Clone, Default)]
pub struct App {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub exec: String,
    pub wm_class: String,
    pub path: PathBuf,
    pub hidden: bool,
}

/// Parse the `[Desktop Entry]` group of a desktop file into key/value pairs.
pub fn parse_desktop(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    out
}

/// The program an `Exec=` line runs, without field codes or wrappers.
pub fn exec_program(exec: &str) -> String {
    let mut words = exec.split_whitespace().filter(|w| !w.starts_with('%'));
    let mut first = words.next().unwrap_or("");
    while first == "env" || first.contains('=') || first == "uwsm-app" || first == "--" {
        first = words.next().unwrap_or("");
    }
    first.trim_matches('"').rsplit('/').next().unwrap_or(first).to_string()
}

fn dirs() -> Vec<PathBuf> {
    let mut d = vec![paths::home().join(".local/share/applications")];
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for p in data.split(':').filter(|p| !p.is_empty()) {
        d.push(Path::new(p).join("applications"));
    }
    d.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    d.push(paths::home().join(".local/share/flatpak/exports/share/applications"));
    d
}

fn load() -> Vec<App> {
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut apps = Vec::new();
    for dir in dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let id = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            // Earlier directories win (the user's own overrides).
            if seen.insert(id.clone(), ()).is_some() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let kv = parse_desktop(&text);
            if kv.get("Type").map(String::as_str) != Some("Application") {
                continue;
            }
            apps.push(App {
                id,
                name: kv.get("Name").cloned().unwrap_or_default(),
                icon: kv.get("Icon").cloned().unwrap_or_default(),
                exec: kv.get("Exec").cloned().unwrap_or_default(),
                wm_class: kv.get("StartupWMClass").cloned().unwrap_or_default(),
                path,
                hidden: kv.get("NoDisplay").is_some_and(|v| v == "true") || kv.get("Hidden").is_some_and(|v| v == "true"),
            });
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

struct Index {
    apps: Rc<Vec<App>>,
    by_key: HashMap<String, usize>,
}

thread_local! {
    static INDEX: Index = {
        let apps = load();
        let mut by_key = HashMap::new();
        for (i, a) in apps.iter().enumerate() {
            for key in [a.wm_class.to_lowercase(), a.id.to_lowercase(), exec_program(&a.exec).to_lowercase()] {
                if !key.is_empty() {
                    by_key.entry(key).or_insert(i);
                }
            }
            // Reverse-DNS ids: also match on the last part (org.gnome.Nautilus -> nautilus).
            if let Some(last) = a.id.rsplit('.').next() {
                by_key.entry(last.to_lowercase()).or_insert(i);
            }
        }
        Index { apps: Rc::new(apps), by_key }
    };
}

pub fn all() -> Rc<Vec<App>> {
    INDEX.with(|i| i.apps.clone())
}

/// Find the app for a window class or process name.
pub fn lookup(key: &str) -> Option<App> {
    let k = key.to_lowercase();
    if k.is_empty() {
        return None;
    }
    INDEX.with(|i| i.by_key.get(&k).map(|&n| i.apps[n].clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entry_group_only() {
        let text = "[Desktop Entry]\nName=Files\nExec=nautilus --new-window %U\n[Desktop Action x]\nName=Other\n";
        let kv = parse_desktop(text);
        assert_eq!(kv["Name"], "Files");
        assert_eq!(exec_program(&kv["Exec"]), "nautilus");
    }

    #[test]
    fn strips_wrappers() {
        assert_eq!(exec_program("env FOO=1 /usr/bin/foo --bar"), "foo");
        assert_eq!(exec_program("uwsm-app -- signal-desktop"), "signal-desktop");
    }
}
