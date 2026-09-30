use crate::startup::{self, Unit};
use crate::widgets::{self, Page};
use crate::{cmd, window};
use gtk::pango;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Show {
    All,
    Running,
    Failed,
}

type Chip<'a> = (&'a str, Rc<dyn Fn()>);

struct State {
    user: Cell<bool>,
    show: Cell<Show>,
    query: RefCell<String>,
    units: RefCell<Vec<Unit>>,
    list: gtk::ListBox,
    banner: gtk::Box,
    count: gtk::Label,
}

fn load(st: &Rc<State>) {
    let user = st.user.get();
    let st = st.clone();
    st.count.set_text("Loading…");
    cmd::background(
        move || startup::units(user),
        move |units| {
            if st.user.get() != user {
                return;
            }
            *st.units.borrow_mut() = units;
            populate(&st);
        },
    );
}

fn state_tag(u: &Unit) -> gtk::Label {
    if u.failed() {
        widgets::tag("Failed", "danger")
    } else if u.running() {
        widgets::tag(if u.sub == "running" { "Running" } else { &u.sub }, "accent")
    } else {
        widgets::tag("Stopped", "")
    }
}

fn populate(st: &Rc<State>) {
    while let Some(c) = st.list.first_child() {
        st.list.remove(&c);
    }
    let units = st.units.borrow();
    let failed = units.iter().filter(|u| u.failed()).count();
    st.banner.set_visible(failed > 0);
    if let Some(l) = st.banner.last_child().and_downcast::<gtk::Label>() {
        l.set_text(&format!("{failed} service(s) failed. Open one to see its logs."));
    }
    for u in units.iter() {
        st.list.append(&unit_row(st, u));
    }
    st.list.invalidate_filter();
    refresh_count(st);
}

fn refresh_count(st: &State) {
    let units = st.units.borrow();
    let shown = units.iter().filter(|u| visible(st, u)).count();
    st.count.set_text(&format!("{shown} of {}", units.len()));
}

fn visible(st: &State, u: &Unit) -> bool {
    let q = st.query.borrow();
    let show = match st.show.get() {
        Show::All => true,
        Show::Running => u.running(),
        Show::Failed => u.failed(),
    };
    show && (q.is_empty() || u.name.to_lowercase().contains(q.as_str()) || u.description.to_lowercase().contains(q.as_str()))
}

fn systemctl(st: &Rc<State>, verb: &str, unit: &str) {
    let user = st.user.get();
    let mut args: Vec<String> = if user {
        vec!["systemctl".into(), "--user".into()]
    } else {
        vec!["pkexec".into(), "systemctl".into(), "--system".into()]
    };
    args.push(verb.into());
    args.push(unit.into());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (st2, verb, unit) = (st.clone(), verb.to_string(), unit.to_string());
    cmd::run_async(&refs, move |r| {
        match r {
            Ok(_) => window::toast(&format!("{}: {verb} done", unit.trim_end_matches(".service"))),
            Err(e) => window::toast(&format!("Couldn't {verb} {unit}: {e}")),
        }
        load(&st2);
    });
}

fn unit_row(st: &Rc<State>, u: &Unit) -> gtk::ListBoxRow {
    let row = widgets::hbox(12);
    row.add_css_class("settings-option");
    let text = widgets::vbox(2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let head = widgets::hbox(8);
    let t = widgets::label(&startup::unescape(u.name.trim_end_matches(".service")), "settings-option-title");
    t.set_ellipsize(pango::EllipsizeMode::End);
    t.set_tooltip_text(Some(&u.name));
    head.append(&t);
    head.append(&state_tag(u));
    if !u.file_state.is_empty() {
        head.append(&widgets::tag(&u.file_state, ""));
    }
    text.append(&head);
    if !u.description.is_empty() {
        let d = widgets::label(&u.description, "settings-option-description");
        d.set_ellipsize(pango::EllipsizeMode::End);
        text.append(&d);
    }
    row.append(&text);

    let menu = gtk::MenuButton::new();
    menu.set_icon_name("view-more-symbolic");
    menu.set_tooltip_text(Some("Actions"));
    menu.set_valign(gtk::Align::Center);
    menu.add_css_class("flat");
    let pop = gtk::Popover::new();
    let actions = widgets::vbox(2);
    let mut verbs: Vec<(&str, &str)> = Vec::new();
    if u.running() {
        verbs.push(("Restart", "restart"));
        verbs.push(("Stop", "stop"));
    } else {
        verbs.push(("Start", "start"));
    }
    match u.file_state.as_str() {
        "enabled" => verbs.push(("Disable at boot", "disable")),
        "disabled" => verbs.push(("Enable at boot", "enable")),
        _ => {}
    }
    for (label, verb) in verbs {
        let b = gtk::Button::with_label(label);
        b.add_css_class("flat");
        let (st2, name, pop2) = (st.clone(), u.name.clone(), pop.clone());
        b.connect_clicked(move |_| {
            pop2.popdown();
            systemctl(&st2, verb, &name);
        });
        actions.append(&b);
    }
    let logs = gtk::Button::with_label("View logs");
    logs.add_css_class("flat");
    let (name, user, pop2) = (u.name.clone(), st.user.get(), pop.clone());
    logs.connect_clicked(move |_| {
        pop2.popdown();
        show_logs(&name, user);
    });
    actions.append(&logs);
    pop.set_child(Some(&actions));
    menu.set_popover(Some(&pop));
    row.append(&menu);

    let r = gtk::ListBoxRow::new();
    r.set_activatable(false);
    r.set_child(Some(&row));
    r
}

fn show_logs(unit: &str, user: bool) {
    let (dialog, card) = widgets::dialog(&format!("Logs · {}", startup::unescape(unit.trim_end_matches(".service"))), 820);
    dialog.set_default_height(560);
    let view = gtk::TextView::new();
    view.set_editable(false);
    view.set_monospace(true);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.add_css_class("code-block");
    view.buffer().set_text("Loading…");
    let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&view).build();
    card.append(&scroll);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    let close = gtk::Button::with_label("Close");
    let d = dialog.clone();
    close.connect_clicked(move |_| d.close());
    buttons.append(&close);
    card.append(&buttons);
    dialog.present();
    let scope = if user { "--user" } else { "--system" };
    let v = view.clone();
    cmd::run_async(&["journalctl", scope, "-u", unit, "-n", "300", "--no-pager", "-o", "short-iso"], move |r| {
        let text = match r {
            Ok(t) if t.trim().is_empty() => "No log entries.".to_string(),
            Ok(t) => t,
            Err(e) => format!("Couldn't read the journal: {e}"),
        };
        v.buffer().set_text(&text);
        // Newest at the bottom; start there.
        let mut end = v.buffer().end_iter();
        v.scroll_to_iter(&mut end, 0.0, false, 0.0, 1.0);
    });
}

pub fn build(page: &Page) {
    let toolbar = widgets::hbox(8);
    toolbar.add_css_class("toolbar");
    let scope_box = widgets::hbox(6);
    let show_box = widgets::hbox(6);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Filter services"));
    search.set_hexpand(true);
    let count = widgets::label("", "dim");
    count.add_css_class("mono");
    toolbar.append(&scope_box);
    toolbar.append(&show_box);
    toolbar.append(&search);
    toolbar.append(&count);
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Refresh"));
    toolbar.append(&refresh);
    page.body.append(&toolbar);

    let banner = widgets::banner("", true);
    banner.set_visible(false);
    page.body.append(&banner);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("service-list");
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&list)
        .build();
    page.body.append(&scroll);

    let st = Rc::new(State {
        user: Cell::new(true),
        show: Cell::new(Show::All),
        query: RefCell::new(String::new()),
        units: RefCell::new(Vec::new()),
        list: list.clone(),
        banner,
        count,
    });

    let s2 = st.clone();
    list.set_filter_func(move |row| {
        let units = s2.units.borrow();
        units.get(row.index() as usize).is_some_and(|u| visible(&s2, u))
    });

    let chip_group = |container: &gtk::Box, items: Vec<Chip>| {
        let buttons: Rc<RefCell<Vec<gtk::Button>>> = Rc::default();
        for (i, (label, act)) in items.into_iter().enumerate() {
            let b = gtk::Button::with_label(label);
            b.add_css_class("chip");
            if i == 0 {
                b.add_css_class("selected");
            }
            let bs = buttons.clone();
            b.connect_clicked(move |me| {
                for x in bs.borrow().iter() {
                    x.remove_css_class("selected");
                }
                me.add_css_class("selected");
                act();
            });
            container.append(&b);
            buttons.borrow_mut().push(b);
        }
    };
    let (a, b) = (st.clone(), st.clone());
    chip_group(
        &scope_box,
        vec![
            (
                "Yours",
                Rc::new(move || {
                    a.user.set(true);
                    load(&a);
                }),
            ),
            (
                "System",
                Rc::new(move || {
                    b.user.set(false);
                    load(&b);
                }),
            ),
        ],
    );
    let mk = |show: Show| {
        let s = st.clone();
        Rc::new(move || {
            s.show.set(show);
            s.list.invalidate_filter();
            refresh_count(&s);
        }) as Rc<dyn Fn()>
    };
    chip_group(&show_box, vec![("All", mk(Show::All)), ("Running", mk(Show::Running)), ("Failed", mk(Show::Failed))]);

    let s2 = st.clone();
    search.connect_search_changed(move |e| {
        *s2.query.borrow_mut() = e.text().trim().to_lowercase();
        s2.list.invalidate_filter();
        refresh_count(&s2);
    });
    let s2 = st.clone();
    refresh.connect_clicked(move |_| load(&s2));
    load(&st);
}
