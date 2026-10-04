//! Desktop notifications when something needs a look: a hot CPU, memory running
//! out, a process stuck at high CPU, or a service that failed. They run while
//! Tasks is open (on screen or not), and each kind waits a while before repeating.

use crate::sampler::Snapshot;
use crate::{cmd, fmt, live, prefs};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// How long one alert stays quiet after it fires.
const COOLDOWN: Duration = Duration::from_secs(300);

#[derive(Default)]
struct Alerts {
    /// When each alert (by key) last fired.
    fired: HashMap<String, Instant>,
    /// Since when each pid has been over the CPU limit.
    busy_since: HashMap<i32, Instant>,
    /// Failed units seen at the last check; None before the first check.
    failed: Option<HashSet<String>>,
}

thread_local! {
    static ALERTS: RefCell<Alerts> = RefCell::new(Alerts::default());
}

pub fn start() {
    live::on_every(check);
    glib::timeout_add_seconds_local(60, || {
        if prefs::get().alert_services {
            check_services();
        } else {
            ALERTS.with(|a| a.borrow_mut().failed = None);
        }
        glib::ControlFlow::Continue
    });
    if prefs::get().alert_services {
        check_services();
    }
}

/// Send `body` unless the alert `key` fired recently. `section` opens on click.
fn notify(key: &str, title: &str, body: &str, section: &str) {
    let due = ALERTS.with(|a| {
        let mut a = a.borrow_mut();
        let due = a.fired.get(key).is_none_or(|t| t.elapsed() >= COOLDOWN);
        if due {
            a.fired.insert(key.to_string(), Instant::now());
        }
        due
    });
    if !due {
        return;
    }
    let Some(app) = gio::Application::default() else { return };
    let n = gio::Notification::new(title);
    n.set_body(Some(body));
    n.set_default_action_and_target_value("app.show", Some(&section.to_variant()));
    app.send_notification(Some(key), &n);
}

fn check(s: &Snapshot) {
    let p = prefs::get();
    if p.alert_temp
        && let Some(t) = s.sensors.cpu_temp
        && t >= p.warn_temp
    {
        notify("temp", "CPU is hot", &format!("The CPU is at {}.", fmt::temp(t)), "sensors");
    }
    if p.alert_mem && s.mem.total > 0 {
        let used = s.mem.used as f64 / s.mem.total as f64 * 100.0;
        if used >= p.warn_mem {
            let body = format!("{} in use, {} available.", fmt::pct(used), fmt::bytes(s.mem.available as f64));
            notify("mem", "Memory is running low", &body, "memory");
        }
    }
    if !p.alert_proc {
        ALERTS.with(|a| a.borrow_mut().busy_since.clear());
        return;
    }
    // Needs per-process data, which stops while the window rests hidden.
    let Some(procs) = &s.procs else { return };
    let now = Instant::now();
    let mut due = Vec::new();
    ALERTS.with(|a| {
        let mut a = a.borrow_mut();
        let over: HashSet<i32> = procs.iter().filter(|q| q.cpu >= p.alert_proc_cpu).map(|q| q.pid).collect();
        a.busy_since.retain(|pid, _| over.contains(pid));
        for q in procs.iter().filter(|q| over.contains(&q.pid)) {
            let since = *a.busy_since.entry(q.pid).or_insert(now);
            if now.duration_since(since).as_secs() >= p.alert_proc_secs {
                due.push((q.pid, q.name.clone(), q.cpu));
            }
        }
    });
    for (pid, name, cpu) in due {
        let body =
            format!("{name} (PID {pid}) has used {} CPU for over {}.", fmt::pct(cpu), fmt::duration(p.alert_proc_secs as f64));
        notify(&format!("proc.{pid}"), "A process is keeping the CPU busy", &body, "processes");
    }
}

/// `systemctl list-units --state=failed -o json` as unit names.
fn parse_failed(json: &str) -> HashSet<String> {
    serde_json::from_str::<Vec<serde_json::Value>>(json)
        .unwrap_or_default()
        .iter()
        .filter_map(|u| u["unit"].as_str().map(String::from))
        .collect()
}

fn check_services() {
    cmd::background(
        || {
            let mut out = HashSet::new();
            for scope in ["--user", "--system"] {
                let json = cmd::output(&["systemctl", scope, "list-units", "--state=failed", "-o", "json", "--no-pager"])
                    .unwrap_or_default();
                out.extend(parse_failed(&json).into_iter().map(|u| format!("{scope} {u}")));
            }
            out
        },
        |now: HashSet<String>| {
            let new: Vec<String> = ALERTS.with(|a| {
                let mut a = a.borrow_mut();
                // The first check only learns what was already failed.
                let new = a.failed.as_ref().map(|old| now.difference(old).cloned().collect()).unwrap_or_default();
                a.failed = Some(now);
                new
            });
            for entry in new {
                let unit = entry.split_once(' ').map_or(entry.as_str(), |(_, u)| u);
                let name = crate::startup::unescape(unit.trim_end_matches(".service"));
                notify(&format!("unit.{entry}"), "A service failed", &format!("{name} stopped with an error."), "services");
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_failed_units() {
        let got = parse_failed(r#"[{"unit":"a.service","load":"loaded","active":"failed"},{"unit":"b.timer"}]"#);
        assert_eq!(got, HashSet::from(["a.service".to_string(), "b.timer".to_string()]));
        assert!(parse_failed("").is_empty());
    }
}
