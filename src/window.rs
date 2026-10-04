//! The main window: navigation sidebar with search, and a stack of
//! section pages that are built the first time they're shown.

use crate::sections::{self, Section};
use crate::widgets::{self, SEARCH};
use crate::{live, prefs, theme};
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

struct Ui {
    window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    nav_items: HashMap<&'static str, gtk::Button>,
    nav_groups: Vec<(gtk::Label, Vec<&'static str>)>,
    pages: HashMap<&'static str, gtk::ScrolledWindow>,
    sections: Vec<Section>,
    current: &'static str,
    /// The one toast, reused so quick messages replace each other.
    toast: gtk::Label,
    toast_timer: Option<glib::SourceId>,
}

thread_local! {
    static UI: RefCell<Option<Rc<RefCell<Ui>>>> = const { RefCell::new(None) };
}

fn ui() -> Option<Rc<RefCell<Ui>>> {
    UI.with(|u| u.borrow().clone())
}

pub fn present(app: &gtk::Application, section: Option<&str>) {
    if let Some(ui) = ui() {
        let window = ui.borrow().window.clone();
        if let Some(s) = section {
            navigate(s);
        }
        window.present();
        return;
    }
    theme::install();
    install_icons();
    live::start();
    crate::alerts::start();
    build(app);
    let start = section.map(String::from).unwrap_or_else(|| prefs::get().last_section);
    navigate(&start);
    // Developer aid: TASKS_SNAPSHOT=/path.png renders the window to a PNG
    // (invisibly) and quits, so layouts can be checked without a visible window.
    if let Some(out) = std::env::var_os("TASKS_SNAPSHOT") {
        snapshot_and_quit(app, std::path::PathBuf::from(out));
        return;
    }
    if let Some(ui) = ui() {
        ui.borrow().window.present();
    }
}

/// Our own symbolic icons (the icon theme may lack a CPU or memory glyph).
/// They're written to the cache once and added to the icon search path.
fn install_icons() {
    const ICONS: &[(&str, &str)] = &[
        ("tasks-cpu-symbolic.svg", include_str!("../data/icons/tasks-cpu-symbolic.svg")),
        ("tasks-memory-symbolic.svg", include_str!("../data/icons/tasks-memory-symbolic.svg")),
        ("tasks-gpu-symbolic.svg", include_str!("../data/icons/tasks-gpu-symbolic.svg")),
        ("tasks-thermometer-symbolic.svg", include_str!("../data/icons/tasks-thermometer-symbolic.svg")),
    ];
    let dir = crate::paths::cache_dir().join("icons");
    for (name, svg) in ICONS {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*svg) {
            let _ = crate::cmd::atomic_write(&path, svg);
        }
    }
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_search_path(&dir);
    }
}

fn build(app: &gtk::Application) {
    let window =
        gtk::ApplicationWindow::builder().application(app).title("Tasks").default_width(1120).default_height(800).build();
    window.add_css_class("tasks-window");
    // No client-side titlebar: Hyprland manages the window.
    window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    window.set_icon_name(Some("io.github.design_nexus.Tasks"));

    let sections = sections::all();

    // ----- Sidebar -----
    let nav = gtk::Box::new(gtk::Orientation::Vertical, 0);
    nav.add_css_class("settings-navigation");
    // Labels inside expand; don't let that widen the sidebar itself.
    nav.set_hexpand(false);
    let heading = widgets::label("TASKS", "menu-heading");
    heading.set_hexpand(true);
    let mut compact_hide: Vec<gtk::Widget> = vec![heading.clone().upcast()];
    nav.append(&nav_head(&heading));

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search pages"));
    search.add_css_class("settings-search");
    nav.append(&search);
    compact_hide.push(search.clone().upcast());

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let mut nav_items = HashMap::new();
    let mut nav_groups: Vec<(gtk::Label, Vec<&'static str>)> = Vec::new();
    let mut last_group = "";
    for s in &sections {
        if s.group != last_group {
            let g = widgets::label(&s.group.to_uppercase(), "nav-group");
            compact_hide.push(g.clone().upcast());
            list.append(&g);
            nav_groups.push((g, Vec::new()));
            last_group = s.group;
        }
        let button = gtk::Button::new();
        button.add_css_class("nav-item");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        content.append(&gtk::Image::from_icon_name(s.icon));
        let l = widgets::label(s.title, "nav-label");
        l.set_hexpand(true);
        compact_hide.push(l.clone().upcast());
        content.append(&l);
        if let Some(readout) = s.readout {
            let r = widgets::label("", "nav-readout");
            r.add_css_class("mono");
            // A long reading ("↓ 12.3 MiB/s") mustn't widen the sidebar.
            r.set_max_width_chars(11);
            r.set_ellipsize(gtk::pango::EllipsizeMode::End);
            compact_hide.push(r.clone().upcast());
            content.append(&r);
            let r2 = r.clone();
            live::on_tick(&r, move |snap| r2.set_text(&readout(snap)));
        }
        button.set_child(Some(&content));
        button.set_tooltip_text(Some(s.description));
        let id = s.id;
        button.connect_clicked(move |_| navigate(id));
        list.append(&button);
        nav_items.insert(s.id, button);
        if let Some(g) = nav_groups.last_mut() {
            g.1.push(s.id);
        }
    }
    let nav_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        // Scrolls with wheel, trackpad and keyboard; no visible scrollbar.
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&list)
        .build();
    nav.append(&nav_scroll);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    footer.add_css_class("nav-footer");
    let version = widgets::label(concat!("Tasks ", env!("CARGO_PKG_VERSION")), "dim");
    version.set_hexpand(true);
    footer.append(&version);
    let pause = pause_button();
    footer.append(&pause);
    nav.append(&footer);
    compact_hide.push(footer.clone().upcast());

    // ----- Content -----
    let stack = gtk::Stack::new();
    stack.add_css_class("settings-content");
    stack.set_hexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(if prefs::get().reduce_motion { 0 } else { 160 });

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.append(&nav);
    body.append(&stack);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&body));
    window.set_child(Some(&overlay));
    let toast = gtk::Label::new(None);
    toast.set_wrap(true);
    toast.set_max_width_chars(70);
    let toast_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toast_box.add_css_class("toast");
    toast_box.append(&toast);
    toast_box.set_halign(gtk::Align::Center);
    toast_box.set_valign(gtk::Align::End);
    toast_box.set_can_target(false);
    toast_box.set_visible(false);
    overlay.add_overlay(&toast_box);

    // ----- Keys -----
    let keys = gtk::EventControllerKey::new();
    let s2 = search.clone();
    let w2 = window.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        match key {
            gdk::Key::f if ctrl => {
                if current() == "processes" {
                    crate::sections::processes::focus_search();
                } else {
                    s2.grab_focus();
                }
                glib::Propagation::Stop
            }
            gdk::Key::k if ctrl => {
                crate::palette::open();
                glib::Propagation::Stop
            }
            gdk::Key::b if ctrl => {
                toggle_sidebar();
                glib::Propagation::Stop
            }
            gdk::Key::q | gdk::Key::w if ctrl => {
                w2.close();
                glib::Propagation::Stop
            }
            gdk::Key::Escape if !s2.text().is_empty() => {
                s2.set_text("");
                glib::Propagation::Stop
            }
            gdk::Key::p if ctrl => {
                toggle_pause();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(keys);
    search.connect_search_changed(|e| filter(&e.text()));
    search.connect_activate(|_| focus_first_hit());

    // Narrow windows (a tiled half-screen) get an icon-only sidebar.
    SIDEBAR.with(|s| *s.borrow_mut() = Some(Sidebar { nav: nav.clone(), hide: compact_hide }));
    let apply_width = {
        let nav = nav.clone();
        move |w: &gtk::ApplicationWindow| {
            let width = if w.width() > 0 { w.width() } else { w.default_width() };
            let narrow = width > 0 && width < 980;
            if narrow == NARROW.with(|n| n.get()) && nav.has_css_class("sized") {
                return;
            }
            nav.add_css_class("sized");
            set_narrow(narrow);
            apply_compact(narrow || prefs::get().sidebar_collapsed);
        }
    };
    let aw = apply_width.clone();
    window.connect_default_width_notify(move |w| aw(w));
    let aw = apply_width.clone();
    window.connect_realize(move |w| aw(w));
    // Tiled windows are resized by the compositor; watch the real size too.
    let w2 = window.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
        apply_width(&w2);
        glib::ControlFlow::Continue
    });

    // Hidden (another workspace, or covered in a monocle layout): stop the
    // expensive per-process and GPU sampling until it's back.
    window.connect_suspended_notify(|w| {
        live::set_detail(!(w.is_suspended() && prefs::get().pause_hidden));
    });

    let ui = Ui { window, stack, nav_items, nav_groups, pages: HashMap::new(), sections, current: "", toast, toast_timer: None };
    UI.with(|u| *u.borrow_mut() = Some(Rc::new(RefCell::new(ui))));
}

/// The parts of the sidebar that change when it collapses to icons.
struct Sidebar {
    nav: gtk::Box,
    /// Hidden in the icon-only sidebar.
    hide: Vec<gtk::Widget>,
}

thread_local! {
    static SIDEBAR: RefCell<Option<Sidebar>> = const { RefCell::new(None) };
    static NARROW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The button that collapses the sidebar to icons, beside the app heading.
fn nav_head(heading: &gtk::Label) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("nav-head");
    row.append(heading);
    let button = gtk::Button::from_icon_name("sidebar-show-symbolic");
    button.add_css_class("nav-collapse");
    button.set_tooltip_text(Some("Collapse or expand the sidebar (Ctrl+B)"));
    button.set_valign(gtk::Align::Center);
    button.connect_clicked(|_| toggle_sidebar());
    row.append(&button);
    row
}

/// The sidebar shows only icons: hide the labels, centre the icons and the toggle.
fn apply_compact(compact: bool) {
    SIDEBAR.with(|s| {
        let s = s.borrow();
        let Some(s) = s.as_ref() else { return };
        if compact {
            s.nav.add_css_class("compact");
        } else {
            s.nav.remove_css_class("compact");
        }
        for w in &s.hide {
            w.set_visible(!compact);
        }
        centre_icons(s.nav.upcast_ref(), compact);
    });
}

fn centre_icons(w: &gtk::Widget, compact: bool) {
    if w.has_css_class("nav-item")
        && let Some(content) = w.downcast_ref::<gtk::Button>().and_then(|b| b.child())
    {
        content.set_halign(if compact { gtk::Align::Center } else { gtk::Align::Fill });
    }
    if w.has_css_class("nav-collapse") {
        w.set_halign(if compact { gtk::Align::Center } else { gtk::Align::End });
        w.set_hexpand(compact);
    }
    let mut child = w.first_child();
    while let Some(c) = child {
        centre_icons(&c, compact);
        child = c.next_sibling();
    }
}

pub fn toggle_sidebar() {
    prefs::update(|p| p.sidebar_collapsed = !p.sidebar_collapsed);
    apply_compact(NARROW.with(|n| n.get()) || prefs::get().sidebar_collapsed);
}

fn set_narrow(narrow: bool) {
    NARROW.with(|n| n.set(narrow));
    let Some(ui) = ui() else { return };
    for page in ui.borrow().pages.values() {
        mark_page(page, narrow);
    }
}

fn mark_page(page: &gtk::ScrolledWindow, narrow: bool) {
    if let Some(body) = page.child().and_then(|v| v.first_child()) {
        if narrow {
            body.add_css_class("narrow");
        } else {
            body.remove_css_class("narrow");
        }
    }
}

fn ensure_built(id: &'static str) {
    let Some(ui) = ui() else { return };
    if ui.borrow().pages.contains_key(id) {
        return;
    }
    let section = {
        let u = ui.borrow();
        u.sections.iter().find(|s| s.id == id).map(|s| (s.id, s.title, s.description, (s.files)(), s.build, s.fill))
    };
    let Some((sid, title, description, files, build, fill)) = section else { return };
    let page = widgets::page(sid, title, description, &files);
    if fill {
        page.fill();
    }
    build(&page);
    mark_page(&page.root, NARROW.with(|n| n.get()));
    let stack = ui.borrow().stack.clone();
    stack.add_named(&page.root, Some(sid));
    ui.borrow_mut().pages.insert(sid, page.root);
}

pub fn navigate(id: &str) {
    let Some(ui) = ui() else { return };
    let resolved = {
        let u = ui.borrow();
        u.sections.iter().find(|s| s.id == id).or_else(|| u.sections.first()).map(|s| s.id)
    };
    let Some(id) = resolved else { return };
    ensure_built(id);
    let mut u = ui.borrow_mut();
    if let Some(prev) = u.nav_items.get(u.current) {
        prev.remove_css_class("active");
    }
    if let Some(b) = u.nav_items.get(id) {
        b.add_css_class("active");
    }
    u.stack.set_visible_child_name(id);
    u.current = id;
    drop(u);
    prefs::update(|p| p.last_section = id.to_string());
}

/// Rebuild a section page from scratch (after a change that alters its layout).
pub fn rebuild(id: &'static str) {
    let Some(ui) = ui() else { return };
    let old = ui.borrow_mut().pages.remove(id);
    if let Some(old) = old {
        SEARCH.with(|s| s.borrow_mut().retain(|item| item.section != id));
        ui.borrow().stack.remove(&old);
    }
    let current = ui.borrow().current;
    ensure_built(id);
    if current == id {
        ui.borrow().stack.set_visible_child_name(id);
    }
}

fn filter(query: &str) {
    let Some(ui) = ui() else { return };
    let q = query.trim().to_lowercase();
    let terms: Vec<&str> = q.split_whitespace().collect();

    if !terms.is_empty() {
        let ids: Vec<&'static str> = ui.borrow().sections.iter().map(|s| s.id).collect();
        for id in ids {
            ensure_built(id);
        }
    }

    let mut section_hits: HashMap<String, usize> = HashMap::new();
    let title_hits: Vec<&'static str> = {
        let u = ui.borrow();
        u.sections
            .iter()
            .filter(|s| {
                let hay = format!("{} {} {}", s.title, s.description, s.keywords).to_lowercase();
                !terms.is_empty() && terms.iter().all(|t| hay.contains(t))
            })
            .map(|s| s.id)
            .collect()
    };

    SEARCH.with(|s| {
        let items = s.borrow();
        let mut groups_visible: HashMap<gtk::Widget, bool> = HashMap::new();
        for item in items.iter() {
            item.row.remove_css_class("search-hit");
            let hit = !terms.is_empty() && terms.iter().all(|t| item.text.contains(t));
            let whole_section = title_hits.iter().any(|id| *id == item.section);
            let show = terms.is_empty() || hit || whole_section;
            item.row.set_visible(show);
            if hit {
                *section_hits.entry(item.section.clone()).or_default() += 1;
            }
            if let Some(g) = &item.group {
                let e = groups_visible.entry(g.clone()).or_insert(false);
                *e |= show;
            }
        }
        for (g, visible) in groups_visible {
            g.set_visible(visible);
        }
    });

    let u = ui.borrow();
    let mut first_match: Option<&'static str> = None;
    for s in &u.sections {
        let Some(button) = u.nav_items.get(s.id) else { continue };
        let visible = terms.is_empty() || section_hits.contains_key(s.id) || title_hits.contains(&s.id);
        button.set_visible(visible);
        if visible && first_match.is_none() && !terms.is_empty() {
            first_match = Some(s.id);
        }
    }
    for (label, ids) in &u.nav_groups {
        label.set_visible(ids.iter().any(|id| u.nav_items.get(id).is_some_and(|b| b.is_visible())));
    }
    let current = u.current;
    let current_visible = u.nav_items.get(current).is_some_and(|b| b.is_visible());
    drop(u);
    if let Some(first) = first_match
        && (!current_visible || !section_hits.contains_key(current))
    {
        navigate(first);
    }
    highlight_first_hit(&terms);
}

fn highlight_first_hit(terms: &[&str]) {
    if terms.is_empty() {
        return;
    }
    let Some(ui) = ui() else { return };
    let current = ui.borrow().current;
    let row = SEARCH.with(|s| {
        s.borrow().iter().find(|i| i.section == current && terms.iter().all(|t| i.text.contains(t))).map(|i| i.row.clone())
    });
    if let Some(row) = row {
        row.add_css_class("search-hit");
        scroll_to(&row);
    }
}

fn scroll_to(row: &gtk::Widget) {
    let Some(ui) = ui() else { return };
    let current = ui.borrow().current;
    let Some(page) = ui.borrow().pages.get(current).cloned() else { return };
    let row = row.clone();
    glib::idle_add_local_once(move || {
        if let Some(child) = page.child()
            && let Some(p) = row.compute_point(&child, &gtk::graphene::Point::new(0.0, 0.0))
        {
            let adj = page.vadjustment();
            adj.set_value((p.y() as f64 - 80.0).max(0.0));
        }
    });
}

fn focus_first_hit() {
    let Some(ui) = ui() else { return };
    let current = ui.borrow().current;
    let row = SEARCH
        .with(|s| s.borrow().iter().find(|i| i.section == current && i.row.has_css_class("search-hit")).map(|i| i.row.clone()));
    if let Some(row) = row {
        row.child_focus(gtk::DirectionType::TabForward);
    }
}

/// Show a short message at the bottom of the window. A newer message replaces
/// the one showing and restarts its timer.
pub fn toast(message: &str) {
    let Some(ui) = ui() else {
        eprintln!("tasks: {message}");
        return;
    };
    let mut u = ui.borrow_mut();
    if let Some(t) = u.toast_timer.take() {
        t.remove();
    }
    u.toast.set_text(message);
    let bx = u.toast.parent();
    if let Some(bx) = &bx {
        bx.set_visible(true);
    }
    let weak = Rc::downgrade(&ui);
    u.toast_timer = Some(glib::timeout_add_local_once(std::time::Duration::from_millis(3500), move || {
        if let Some(ui) = weak.upgrade() {
            let mut u = ui.borrow_mut();
            u.toast_timer = None;
            if let Some(bx) = u.toast.parent() {
                bx.set_visible(false);
            }
        }
    }));
}

pub fn current() -> &'static str {
    ui().map(|u| u.borrow().current).unwrap_or("")
}

thread_local! {
    static PAUSE_BUTTONS: RefCell<Vec<gtk::Button>> = const { RefCell::new(Vec::new()) };
}

/// The live/paused pill in the sidebar footer.
fn pause_button() -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("chip");
    b.add_css_class("live-pill");
    b.set_tooltip_text(Some("Pause or resume updates (Ctrl+P)"));
    b.connect_clicked(|_| toggle_pause());
    PAUSE_BUTTONS.with(|p| p.borrow_mut().push(b.clone()));
    refresh_pause();
    b
}

pub fn refresh_pause() {
    let paused = live::paused();
    let secs = prefs::get().interval_ms as f64 / 1000.0;
    let label = if paused { "Paused".to_string() } else { format!("Live · {secs} s") };
    PAUSE_BUTTONS.with(|p| {
        for b in p.borrow().iter() {
            b.set_label(&label);
            if paused {
                b.add_css_class("paused");
            } else {
                b.remove_css_class("paused");
            }
        }
    });
}

pub fn toggle_pause() {
    live::set_paused(!live::paused());
    refresh_pause();
}

pub fn window() -> Option<gtk::ApplicationWindow> {
    ui().map(|u| u.borrow().window.clone())
}

fn snapshot_and_quit(app: &gtk::Application, out: std::path::PathBuf) {
    let Some(ui) = ui() else { return };
    let window = ui.borrow().window.clone();
    window.set_opacity(0.01);
    // A distinct title lets a window rule float it at a set size for screenshots.
    window.set_title(Some("Tasks snapshot"));
    window.set_default_size(
        std::env::var("TASKS_SNAPSHOT_W").ok().and_then(|v| v.parse().ok()).unwrap_or(1120),
        std::env::var("TASKS_SNAPSHOT_H").ok().and_then(|v| v.parse().ok()).unwrap_or(820),
    );
    window.present();
    let app = app.clone();
    let delay = std::env::var("TASKS_SNAPSHOT_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(1800);
    glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
        // TASKS_SNAPSHOT_PAGE=1 renders the whole current page, not just what fits.
        let target = if std::env::var_os("TASKS_SNAPSHOT_PAGE").is_some() {
            UI.with(|cell| cell.borrow().clone()).and_then(|u| {
                let u = u.borrow();
                u.pages.get(u.current).and_then(|p| p.child()).and_then(|v| v.first_child())
            })
        } else {
            window.child()
        };
        if let Some(child) = target {
            let paintable = gtk::WidgetPaintable::new(Some(&child));
            let (w, h) = (child.width(), child.height());
            let snapshot = gtk::Snapshot::new();
            snapshot.append_color(&gdk::RGBA::BLACK, &gtk::graphene::Rect::new(0.0, 0.0, w as f32, h as f32));
            paintable.snapshot(&snapshot, w as f64, h as f64);
            if let (Some(node), Some(renderer)) = (snapshot.to_node(), window.renderer()) {
                let texture = renderer.render_texture(node, None);
                match texture.save_to_png(&out) {
                    Ok(()) => println!("snapshot {w}x{h} -> {}", out.display()),
                    Err(e) => eprintln!("snapshot failed: {e}"),
                }
            }
        }
        app.quit();
    });
}
