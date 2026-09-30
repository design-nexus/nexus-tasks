use crate::widgets::{self, Page};
use crate::{live, prefs, theme, window};
use gtk::glib;
use gtk::prelude::*;

pub fn build(page: &Page) {
    // ----- This window -----
    let g = page.group("This window");
    let p = prefs::get();
    let app_themes = theme::all();
    let options: Vec<(String, String)> = app_themes.iter().map(|t| (t.id.clone(), t.name.clone())).collect();
    let (theme_row, theme_dd) = widgets::choice_row(
        "Theme",
        "Dracula, Catppuccin, Tokyo Night, One Dark Pro and more. Add your own in <tt>~/.config/nexus-tasks/themes</tt>.",
        options,
        &p.theme,
        |id| {
            prefs::update(|p| {
                p.theme = id;
                p.mode = prefs::ThemeMode::Theme;
            });
            theme::apply();
        },
    );
    theme_dd.set_sensitive(p.mode == prefs::ThemeMode::Theme || !theme::omarchy_available());

    if theme::omarchy_available() {
        let dd = theme_dd.clone();
        let (r, _) = widgets::switch_row(
            "Follow Omarchy theme",
            "Match the desktop's colours and update live whenever the Omarchy theme changes.",
            p.mode == prefs::ThemeMode::Omarchy,
            move |on| {
                prefs::update(|p| p.mode = if on { prefs::ThemeMode::Omarchy } else { prefs::ThemeMode::Theme });
                dd.set_sensitive(!on);
                theme::apply();
            },
        );
        g.add(&r);
    }
    g.add(&theme_row);

    let swatches = widgets::hbox(4);
    let refresh_swatches = {
        let swatches = swatches.clone();
        move || {
            while let Some(c) = swatches.first_child() {
                swatches.remove(&c);
            }
            let pal = theme::current_palette();
            for c in [&pal.bg, &pal.surface, &pal.muted, &pal.text, &pal.accent, &pal.danger] {
                let s = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                s.add_css_class("swatch");
                let provider = gtk::CssProvider::new();
                provider.load_from_string(&format!("box {{ background: {c}; }}"));
                #[allow(deprecated)]
                s.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_USER);
                swatches.append(&s);
            }
        }
    };
    refresh_swatches();
    let last = std::cell::RefCell::new(theme::current_palette());
    let weak = swatches.downgrade();
    glib::timeout_add_seconds_local(1, move || {
        if weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        let now = theme::current_palette();
        if *last.borrow() != now {
            *last.borrow_mut() = now;
            refresh_swatches();
        }
        glib::ControlFlow::Continue
    });
    g.add(&widgets::row("Current colours", "", Some(swatches.upcast_ref())));

    let (r, _) = widgets::switch_row("Glow", "Soft accent glow around focused and selected elements.", p.glow, |on| {
        prefs::update(|p| p.glow = on);
        theme::apply();
    });
    g.add(&r);
    let (r, _) =
        widgets::switch_row("Reduce motion", "Turn off transitions and animations in this window.", p.reduce_motion, |on| {
            prefs::update(|p| p.reduce_motion = on);
            theme::apply();
        });
    g.add(&r);

    // ----- Monitoring -----
    let g = page.group("Monitoring");
    let (r, _) = widgets::choice_row(
        "Update every",
        "How often numbers and graphs refresh. Faster costs a little more CPU.",
        widgets::opts(&[("500", "0.5 seconds"), ("1000", "1 second"), ("2000", "2 seconds"), ("5000", "5 seconds")]),
        &p.interval_ms.to_string(),
        |v| {
            let ms: u64 = v.parse().unwrap_or(1000);
            prefs::update(|p| p.interval_ms = ms);
            live::set_interval(ms);
            window::refresh_pause();
        },
    );
    g.add(&r);
    let (r, _) = widgets::choice_row(
        "Graph history",
        "How far back the graphs reach.",
        widgets::opts(&[("60", "1 minute"), ("300", "5 minutes"), ("600", "10 minutes")]),
        &p.history_secs.to_string(),
        |v| {
            prefs::update(|p| p.history_secs = v.parse().unwrap_or(60));
            live::trim();
        },
    );
    g.add(&r);
    let (r, _) =
        widgets::switch_row("Network speeds in bits", "Show Mb/s like internet plans do, instead of MiB/s.", p.net_bits, |on| {
            prefs::update(|p| p.net_bits = on)
        });
    g.add(&r);
    let (r, _) = widgets::choice_row(
        "Temperature unit",
        "How temperatures are shown everywhere, graphs included.",
        widgets::opts(&[("c", "Celsius (°C)"), ("f", "Fahrenheit (°F)")]),
        if p.fahrenheit { "f" } else { "c" },
        |v| {
            prefs::update(|p| p.fahrenheit = v == "f");
            // Refresh the readouts now rather than at the next sample.
            live::poke();
        },
    );
    g.add(&r);
    let (r, _) = widgets::switch_row(
        "Rest while hidden",
        "Stop reading every process and the GPU while this window is out of sight. The graphs keep going.",
        p.pause_hidden,
        |on| {
            prefs::update(|p| p.pause_hidden = on);
            if !on {
                live::set_detail(true);
            }
        },
    );
    g.add(&r);

    // ----- Processes -----
    let g = page.group("Processes");
    let (r, _) = widgets::switch_row(
        "Ask before ending tasks",
        "End task and Kill need a second click to confirm.",
        p.confirm_kill,
        |on| prefs::update(|p| p.confirm_kill = on),
    );
    g.add(&r);
    let (r, _) = widgets::switch_row(
        "Show kernel threads",
        "List the kernel's own workers (kworker, ksoftirqd…) in All and Tree.",
        p.kernel_threads,
        |on| {
            prefs::update(|p| p.kernel_threads = on);
            super::processes::refilter();
        },
    );
    g.add(&r);

    // ----- Keyboard -----
    let g = page.group("Keyboard");
    for (keys, what) in [
        (&["Ctrl", "F"][..], "Search pages, or filter processes on the Processes page"),
        (&["Ctrl", "P"][..], "Pause or resume updates"),
        (&["Delete"][..], "End the selected process"),
        (&["Shift", "Delete"][..], "Kill the selected process"),
        (&["Esc"][..], "Clear the search"),
        (&["Ctrl", "Q"][..], "Close"),
    ] {
        let caps = widgets::hbox(4);
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                caps.append(&widgets::label("+", "dim"));
            }
            caps.append(&widgets::label(k, "key-cap"));
        }
        g.add(&widgets::row(what, "", Some(caps.upcast_ref())));
    }
    g.note("To open Tasks with a key, bind <tt>tasks --toggle</tt> in <tt>~/.config/hypr/bindings.lua</tt>, for example Ctrl+Shift+Esc.");
}
