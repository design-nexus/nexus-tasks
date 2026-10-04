//! Things you can do to a process or its window.

use crate::{cmd, live, window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
    Stop,
    Cont,
}

impl Signal {
    fn raw(self) -> i32 {
        match self {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Stop => libc::SIGSTOP,
            Signal::Cont => libc::SIGCONT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Signal::Term => "TERM",
            Signal::Kill => "KILL",
            Signal::Stop => "STOP",
            Signal::Cont => "CONT",
        }
    }

    fn done(self) -> &'static str {
        match self {
            Signal::Term => "Asked to end",
            Signal::Kill => "Killed",
            Signal::Stop => "Paused",
            Signal::Cont => "Resumed",
        }
    }
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Send a signal to each pid. Processes we may not signal are retried through
/// `pkexec` (one password prompt for the lot).
pub fn signal(targets: &[(i32, String)], sig: Signal) {
    let mut denied: Vec<(i32, String)> = Vec::new();
    let mut ok = 0;
    let mut gone = 0;
    for (pid, name) in targets {
        if *pid <= 1 {
            continue;
        }
        if unsafe { libc::kill(*pid, sig.raw()) } == 0 {
            ok += 1;
        } else {
            match errno() {
                libc::EPERM => denied.push((*pid, name.clone())),
                libc::ESRCH => gone += 1,
                _ => {}
            }
        }
    }
    if ok > 0 {
        let what = if targets.len() == 1 { targets[0].1.clone() } else { format!("{ok} processes") };
        window::toast(&format!("{} {what}", sig.done()));
    } else if gone > 0 && denied.is_empty() {
        window::toast("That process has already ended");
    }
    if !denied.is_empty() {
        let mut args: Vec<String> = vec!["pkexec".into(), "kill".into(), format!("-{}", sig.name())];
        args.extend(denied.iter().map(|(p, _)| p.to_string()));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let n = denied.len();
        cmd::run_async(&refs, move |r| match r {
            Ok(_) => window::toast(&format!("{} {n} process(es) as administrator", sig.done())),
            Err(_) => window::toast("Not allowed: that process belongs to another user"),
        });
    }
    live::poke();
}

/// Change the niceness (-20 … 19) of each pid. Raising priority past what we
/// may set is retried through `pkexec` (one password prompt for the lot).
pub fn renice(targets: &[(i32, String)], nice: i32) {
    let mut denied: Vec<i32> = Vec::new();
    let mut ok = 0;
    for (pid, _) in targets {
        if unsafe { libc::setpriority(libc::PRIO_PROCESS, *pid as libc::id_t, nice) } == 0 {
            ok += 1;
        } else if errno() != libc::ESRCH {
            denied.push(*pid);
        }
    }
    let what = |n: usize| if targets.len() == 1 { targets[0].1.clone() } else { format!("{n} processes") };
    if ok > 0 {
        window::toast(&format!("{} now run{} at priority {nice}", what(ok), if targets.len() == 1 { "s" } else { "" }));
    }
    live::poke();
    if denied.is_empty() {
        return;
    }
    let mut args: Vec<String> = vec!["pkexec".into(), "renice".into(), "-n".into(), nice.to_string(), "-p".into()];
    args.extend(denied.iter().map(|p| p.to_string()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let n = denied.len();
    let label = what(n);
    let single = targets.len() == 1;
    cmd::run_async(&refs, move |r| match r {
        Ok(_) => {
            window::toast(&format!("{label} now run{} at priority {nice}", if single { "s" } else { "" }));
            live::poke();
        }
        Err(_) => window::toast("Priority not changed"),
    });
}

fn hypr_eval(lua: String, failure: &'static str) {
    cmd::run_async(&["hyprctl", "eval", &lua], move |r| {
        if r.is_err() || r.as_deref().is_ok_and(|o| o.contains("not found")) {
            window::toast(failure);
        }
    });
}

pub fn focus_window(address: &str) {
    hypr_eval(format!("hl.dispatch(hl.dsp.focus({{ window = \"address:{address}\" }}))"), "That window is gone");
}

/// Politely close a window, the same as Super+W.
pub fn close_window(address: &str) {
    hypr_eval(format!("hl.dispatch(hl.dsp.window.close({{ window = \"address:{address}\" }}))"), "Couldn't close that window");
}

/// Open the folder holding the program's executable.
pub fn open_location(pid: i32) {
    match std::fs::read_link(format!("/proc/{pid}/exe")) {
        Ok(exe) => {
            let dir = exe.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "/".into());
            cmd::spawn(&["xdg-open", &dir]);
        }
        Err(_) => window::toast("Its location isn't readable (another user's process, or a kernel thread)"),
    }
}

pub fn copy(text: &str) {
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::prelude::DisplayExt::clipboard(&display).set_text(text);
        window::toast("Copied");
    }
}

/// Ask where to save graph history, then write it there as CSV.
pub fn export_csv(name: &str, keys: Option<Vec<String>>) {
    let text = live::csv(keys.as_deref());
    let dialog = gtk::FileDialog::builder().title("Export graph history").initial_name(format!("{name}.csv")).modal(true).build();
    dialog.save(window::window().as_ref(), None::<&gtk::gio::Cancellable>, move |res| {
        let Ok(file) = res else { return };
        let Some(path) = gtk::prelude::FileExt::path(&file) else { return };
        match std::fs::write(&path, text) {
            Ok(()) => window::toast(&format!("Saved {}", crate::paths::pretty(&path))),
            Err(e) => window::toast(&format!("Couldn't save: {e}")),
        }
    });
}
