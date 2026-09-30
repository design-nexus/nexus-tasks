//! Tasks — a task manager for Omarchy.

mod actions;
mod apps;
mod cmd;
mod fmt;
mod graph;
mod live;
mod paths;
mod prefs;
mod sampler;
mod sections;
mod startup;
mod theme;
mod widgets;
mod window;

use gtk::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "io.github.design_nexus.Tasks";

const USAGE: &str = "Usage: tasks [--section ID] [--toggle]\n\
\n\
  --section ID   open (or switch the open window) to a page: overview, processes, cpu,\n\
                 memory, storage, network, gpu, sensors, startup, services, settings\n\
  --toggle       close the window if it's open, otherwise open it (for a keybinding)\n";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return glib::ExitCode::SUCCESS;
    }

    // GTK's Vulkan renderer enumerates every GPU at startup, which wakes a
    // sleeping discrete GPU on hybrid laptops. GL renders only on the one in use.
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: still single-threaded; nothing else reads the environment yet.
        unsafe { std::env::set_var("GSK_RENDERER", "ngl") };
    }

    let app = gtk::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE).build();
    app.connect_command_line(|app, cl| {
        let argv: Vec<String> = cl.arguments().iter().map(|a| a.to_string_lossy().to_string()).collect();
        let section = argv.iter().position(|a| a == "--section").and_then(|i| argv.get(i + 1)).cloned();
        if argv.iter().any(|a| a == "--toggle")
            && let Some(w) = window::window()
        {
            w.close();
            return glib::ExitCode::SUCCESS;
        }
        window::present(app, section.as_deref());
        glib::ExitCode::SUCCESS
    });
    app.run()
}
