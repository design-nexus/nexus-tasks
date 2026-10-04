//! Nexus-style building blocks: pages, groups and option rows.

use crate::{cmd, paths};
use gtk::pango;
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

// ---------- Search registry ----------

pub struct SearchItem {
    pub section: String,
    pub text: String,
    pub row: gtk::Widget,
    pub group: Option<gtk::Widget>,
}

thread_local! {
    static CURRENT_SECTION: RefCell<String> = const { RefCell::new(String::new()) };
    static CURRENT_GROUP: RefCell<Option<gtk::Widget>> = const { RefCell::new(None) };
    pub static SEARCH: RefCell<Vec<SearchItem>> = const { RefCell::new(Vec::new()) };
}

fn register(row: &impl IsA<gtk::Widget>, title: &str, desc: &str, keywords: &str) {
    let section = CURRENT_SECTION.with(|s| s.borrow().clone());
    let group = CURRENT_GROUP.with(|g| g.borrow().clone());
    SEARCH.with(|s| {
        s.borrow_mut().push(SearchItem {
            section,
            text: format!("{title} {desc} {keywords}").to_lowercase(),
            row: row.clone().upcast(),
            group,
        })
    });
}

// ---------- Page / group ----------

pub struct Page {
    pub root: gtk::ScrolledWindow,
    pub body: gtk::Box,
}

pub fn page(section: &str, title: &str, description: &str, files: &[PathBuf]) -> Page {
    CURRENT_SECTION.with(|s| *s.borrow_mut() = section.to_string());
    CURRENT_GROUP.with(|g| *g.borrow_mut() = None);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("settings-page");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("section-header");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("section-title");
    t.set_xalign(0.0);
    let d = gtk::Label::new(Some(description));
    d.add_css_class("section-description");
    d.set_xalign(0.0);
    d.set_wrap(true);
    text.append(&t);
    text.append(&d);
    header.append(&text);
    if !files.is_empty() {
        header.append(&open_config_button(files));
    }
    body.append(&header);

    body.set_hexpand(true);

    let root = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        // Scrolls with wheel, trackpad and keyboard; no visible scrollbar.
        .vscrollbar_policy(gtk::PolicyType::External)
        .child(&body)
        .vexpand(true)
        .build();
    Page { root, body }
}

impl Page {
    /// For pages whose content scrolls itself (tables): the page stops scrolling
    /// and hands its height to the last child.
    pub fn fill(&self) {
        self.root.set_vscrollbar_policy(gtk::PolicyType::Never);
        self.body.set_vexpand(true);
        self.body.add_css_class("fill");
    }

    pub fn group(&self, title: &str) -> Group {
        let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
        wrapper.add_css_class("settings-group");
        if !title.is_empty() {
            let l = gtk::Label::new(Some(&title.to_uppercase()));
            l.add_css_class("group-title");
            l.set_xalign(0.0);
            wrapper.append(&l);
        }
        let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
        wrapper.append(&list);
        self.body.append(&wrapper);
        CURRENT_GROUP.with(|g| *g.borrow_mut() = Some(wrapper.clone().upcast()));
        Group { wrapper, list }
    }
}

pub fn banner(text: &str, warning: bool) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    b.add_css_class("banner");
    if warning {
        b.add_css_class("warning");
    }
    let icon = gtk::Image::from_icon_name(if warning { "dialog-warning-symbolic" } else { "dialog-information-symbolic" });
    icon.set_valign(gtk::Align::Start);
    let l = gtk::Label::new(None);
    l.set_markup(text);
    l.set_wrap(true);
    l.set_xalign(0.0);
    l.set_hexpand(true);
    b.append(&icon);
    b.append(&l);
    b
}

#[derive(Clone)]
pub struct Group {
    pub wrapper: gtk::Box,
    pub list: gtk::Box,
}

impl Group {
    pub fn add(&self, w: &impl IsA<gtk::Widget>) {
        self.list.append(w);
    }

    /// Put a small control at the right end of the group's title line.
    pub fn header_end(&self, w: &impl IsA<gtk::Widget>) {
        let Some(title) = self.wrapper.first_child() else { return };
        self.wrapper.remove(&title);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("group-head");
        title.set_hexpand(true);
        title.set_valign(gtk::Align::Center);
        row.append(&title);
        w.set_valign(gtk::Align::Center);
        row.append(w);
        self.wrapper.prepend(&row);
    }

    pub fn note(&self, text: &str) {
        let l = gtk::Label::new(None);
        l.set_markup(text);
        l.add_css_class("group-note");
        l.set_xalign(0.0);
        l.set_wrap(true);
        // Notes sit just under the group title.
        self.wrapper.insert_child_after(&l, self.wrapper.first_child().as_ref());
    }
}

// ---------- Open config ----------

pub fn open_config_button(files: &[PathBuf]) -> gtk::Widget {
    let make_content = || {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        b.append(&gtk::Image::from_icon_name("text-editor-symbolic"));
        b.append(&gtk::Label::new(Some("Open config")));
        b
    };
    if files.len() == 1 {
        let path = files[0].clone();
        let button = gtk::Button::new();
        button.set_child(Some(&make_content()));
        button.add_css_class("open-config");
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some(&paths::pretty(&path)));
        button.connect_clicked(move |_| cmd::open_in_editor(&path));
        return button.upcast();
    }
    let menu = gtk::MenuButton::new();
    menu.set_child(Some(&make_content()));
    menu.add_css_class("open-config");
    menu.set_valign(gtk::Align::Center);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let popover = gtk::Popover::new();
    for path in files {
        let b = gtk::Button::with_label(&paths::pretty(path));
        b.add_css_class("flat");
        if let Some(label) = b.child().and_downcast::<gtk::Label>() {
            label.set_xalign(0.0);
            label.add_css_class("mono");
        }
        let p = path.clone();
        let pop = popover.clone();
        b.connect_clicked(move |_| {
            pop.popdown();
            cmd::open_in_editor(&p);
        });
        list.append(&b);
    }
    popover.set_child(Some(&list));
    menu.set_popover(Some(&popover));
    menu.upcast()
}

// ---------- Rows ----------

/// An option card: title and description on the left, control on the right.
pub fn row(title: &str, desc: &str, control: Option<&gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("settings-option");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("settings-option-title");
    t.set_xalign(0.0);
    t.set_wrap(true);
    text.append(&t);
    if !desc.is_empty() {
        let d = gtk::Label::new(None);
        d.set_markup(desc);
        d.add_css_class("settings-option-description");
        d.set_xalign(0.0);
        d.set_wrap(true);
        d.set_wrap_mode(pango::WrapMode::WordChar);
        text.append(&d);
    }
    row.append(&text);
    if let Some(c) = control {
        c.set_valign(gtk::Align::Center);
        row.append(c);
    }
    register(&row, title, desc, "");
    row
}

pub fn switch_row(title: &str, desc: &str, active: bool, on_change: impl Fn(bool) + 'static) -> (gtk::Box, gtk::Switch) {
    let sw = gtk::Switch::new();
    sw.set_active(active);
    sw.connect_active_notify(move |s| on_change(s.is_active()));
    let r = row(title, desc, Some(sw.upcast_ref()));
    (r, sw)
}

/// A row with a number field. `on_change` gets each new value.
pub fn spin_row(
    title: &str,
    desc: &str,
    (min, max, step): (f64, f64, f64),
    value: f64,
    on_change: impl Fn(f64) + 'static,
) -> (gtk::Box, gtk::SpinButton) {
    let spin = gtk::SpinButton::with_range(min, max, step);
    spin.set_value(value);
    spin.set_valign(gtk::Align::Center);
    spin.connect_value_changed(move |s| on_change(s.value()));
    (row(title, desc, Some(spin.upcast_ref())), spin)
}

pub fn dropdown(options: &[(String, String)], current: &str) -> gtk::DropDown {
    let labels: Vec<&str> = options.iter().map(|(_, l)| l.as_str()).collect();
    let dd = gtk::DropDown::from_strings(&labels);
    if let Some(i) = options.iter().position(|(id, _)| id == current) {
        dd.set_selected(i as u32);
    } else {
        dd.set_selected(gtk::INVALID_LIST_POSITION);
    }
    dd
}

pub fn opts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

pub fn choice_row(
    title: &str,
    desc: &str,
    options: Vec<(String, String)>,
    current: &str,
    on_change: impl Fn(String) + 'static,
) -> (gtk::Box, gtk::DropDown) {
    let dd = dropdown(&options, current);
    dd.connect_selected_notify(move |d| {
        if let Some((id, _)) = options.get(d.selected() as usize) {
            on_change(id.clone());
        }
    });
    let r = row(title, desc, Some(dd.upcast_ref()));
    (r, dd)
}

pub fn button_row(title: &str, desc: &str, label: &str, on_click: impl Fn(&gtk::Button) + 'static) -> (gtk::Box, gtk::Button) {
    let b = gtk::Button::with_label(label);
    b.connect_clicked(on_click);
    let r = row(title, desc, Some(b.upcast_ref()));
    (r, b)
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    if !class.is_empty() {
        l.add_css_class(class);
    }
    l.set_xalign(0.0);
    l
}

// ---------- Monitoring pieces ----------

/// A card holding a graph: title on the left, a live summary on the right.
pub fn graph_card(title: &str, graph: &impl IsA<gtk::Widget>) -> (gtk::Box, gtk::Label) {
    let card = vbox(10);
    card.add_css_class("graph-card");
    let head = hbox(10);
    let t = label(title, "graph-card-title");
    t.set_hexpand(true);
    t.set_ellipsize(pango::EllipsizeMode::End);
    let summary = label("", "dim");
    summary.add_css_class("mono");
    summary.set_xalign(1.0);
    head.append(&t);
    head.append(&summary);
    card.append(&head);
    card.append(graph);
    register(&card, title, "", "");
    (card, summary)
}

/// A small key/value pair: dim caption over a mono figure.
pub fn kv(key: &str) -> (gtk::Box, gtk::Label) {
    let b = vbox(1);
    b.append(&label(key, "kv-key"));
    let v = label("–", "kv-value");
    v.add_css_class("mono");
    v.set_ellipsize(pango::EllipsizeMode::End);
    b.append(&v);
    (b, v)
}

/// Lay key figures out in a wrapping grid that fits any width.
pub fn kv_flow(keys: &[&str]) -> (gtk::FlowBox, Vec<gtk::Label>) {
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(6);
    flow.set_row_spacing(10);
    flow.set_column_spacing(18);
    let mut values = Vec::new();
    for k in keys {
        let (b, v) = kv(k);
        b.set_size_request(110, -1);
        flow.append(&b);
        values.push(v);
    }
    // Children are focusable by default; these are read-only.
    let mut child = flow.first_child();
    while let Some(c) = child {
        c.set_focusable(false);
        child = c.next_sibling();
    }
    (flow, values)
}

/// Mark a value as past its warning threshold (or not).
pub fn set_warn(w: &impl IsA<gtk::Widget>, over: bool) {
    if over {
        w.add_css_class("warning-text");
    } else {
        w.remove_css_class("warning-text");
    }
}

/// A usage bar with its own threshold: `danger` when `over`.
pub fn warn_bar(bar: &gtk::LevelBar, over: bool) {
    if over {
        bar.add_css_class("warn");
    } else {
        bar.remove_css_class("warn");
    }
}

/// A thin usage bar that turns `danger` above 90%.
pub fn usage_bar() -> gtk::LevelBar {
    let bar = gtk::LevelBar::for_interval(0.0, 1.0);
    for name in ["low", "high", "full"] {
        bar.remove_offset_value(Some(name));
    }
    bar.add_offset_value("ok", 0.9);
    bar.add_offset_value("hot", 1.0);
    bar.set_hexpand(true);
    bar.set_valign(gtk::Align::Center);
    bar
}

/// A spinner with a line of text, for content that's still loading.
pub fn loading(text: &str) -> gtk::Box {
    let b = hbox(10);
    b.add_css_class("loading");
    let spinner = gtk::Spinner::new();
    spinner.start();
    b.append(&spinner);
    b.append(&label(text, "dim"));
    b
}

/// A small pill label ("Stopped", "Omarchy", "Failed").
pub fn tag(text: &str, kind: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("tag");
    if !kind.is_empty() {
        l.add_css_class(kind);
    }
    l.set_valign(gtk::Align::Center);
    l
}

/// A button that needs two clicks: the first arms it ("Click again to …"),
/// the second acts. It disarms itself after a few seconds.
pub fn two_click(label_text: &str, armed_text: &str, act: impl Fn() + 'static) -> gtk::Button {
    two_click_if(label_text, armed_text, || true, act)
}

/// Like `two_click`, but `confirm` is asked on every click; when it says no,
/// one click acts. While armed the button has the `armed` class, and its label
/// may be changed by the caller at any other time.
pub fn two_click_if(
    label_text: &str,
    armed_text: &str,
    confirm: impl Fn() -> bool + 'static,
    act: impl Fn() + 'static,
) -> gtk::Button {
    let b = gtk::Button::with_label(label_text);
    b.add_css_class("destructive-action");
    let idle = Rc::new(RefCell::new(String::new()));
    let armed_label = armed_text.to_string();
    // Each arming gets a number, so an old timer can't disarm a newer one.
    let generation = Rc::new(std::cell::Cell::new(0u32));
    b.connect_clicked(move |b| {
        if b.has_css_class("armed") {
            b.remove_css_class("armed");
            b.set_label(&idle.borrow());
            act();
            return;
        }
        if !confirm() {
            act();
            return;
        }
        *idle.borrow_mut() = b.label().map(|l| l.to_string()).unwrap_or_default();
        b.add_css_class("armed");
        b.set_label(&armed_label);
        generation.set(generation.get().wrapping_add(1));
        let (b2, idle2, gen2, mine) = (b.clone(), idle.clone(), generation.clone(), generation.get());
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || {
            if b2.has_css_class("armed") && gen2.get() == mine {
                b2.remove_css_class("armed");
                b2.set_label(&idle2.borrow());
            }
        });
    });
    b
}

/// A modal card dialog in the app's style. Returns the window and its content box.
pub fn dialog(title: &str, width: i32) -> (gtk::Window, gtk::Box) {
    let dialog = gtk::Window::builder().modal(true).title(title).default_width(width).build();
    if let Some(parent) = crate::window::window() {
        dialog.set_transient_for(Some(&parent));
    }
    dialog.add_css_class("tasks-window");
    dialog.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    let card = vbox(12);
    card.add_css_class("dialog-card");
    card.append(&label(title, "section-title"));
    dialog.set_child(Some(&card));
    let keys = gtk::EventControllerKey::new();
    let d = dialog.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            d.close();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    dialog.add_controller(keys);
    (dialog, card)
}
