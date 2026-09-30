//! The process table. One stable object per pid lives in a `gio::ListStore`;
//! every tick we update their data in Rust, work out order, tree depth and
//! visibility ourselves, and let GTK's sort/filter models follow. Selection and
//! scroll position survive updates because the objects never change identity.

use crate::actions::{self, Signal};
use crate::sampler::Snapshot;
use crate::sampler::procs::Proc;
use crate::widgets::{self, Page};
use crate::{apps, fmt, graph, live, prefs, window};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// ---------- Row data ----------

#[derive(Debug, Clone, Default)]
pub struct Row {
    pub p: Proc,
    pub icon: String,
    pub title: String,
    pub address: String,
    // Computed per tick for the current view.
    pub depth: u32,
    pub has_children: bool,
    pub expanded: bool,
    pub visible: bool,
    pub order: u32,
    /// Summed over the subtree, shown while a group is collapsed.
    pub agg: Option<Agg>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Agg {
    pub cpu: f64,
    pub mem: u64,
    pub disk: f64,
    pub gpu: f64,
    pub threads: u32,
    pub count: u32,
}

impl Row {
    fn cpu(&self) -> f64 {
        self.agg.map_or(self.p.cpu, |a| a.cpu)
    }
    fn mem(&self) -> u64 {
        self.agg.map_or(self.p.rss, |a| a.mem)
    }
    fn disk(&self) -> f64 {
        self.agg.map_or(self.p.read_bps + self.p.write_bps, |a| a.disk)
    }
    fn gpu(&self) -> f64 {
        self.agg.map_or(self.p.gpu, |a| a.gpu)
    }
    fn threads(&self) -> u32 {
        self.agg.map_or(self.p.threads, |a| a.threads)
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ProcObject {
        pub row: RefCell<Row>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ProcObject {
        const NAME: &'static str = "TasksProcObject";
        type Type = super::ProcObject;
    }

    impl ObjectImpl for ProcObject {}
}

glib::wrapper! {
    pub struct ProcObject(ObjectSubclass<imp::ProcObject>);
}

impl ProcObject {
    fn new(row: Row) -> Self {
        let o: Self = glib::Object::new();
        *o.imp().row.borrow_mut() = row;
        o
    }
    pub fn row(&self) -> std::cell::Ref<'_, Row> {
        self.imp().row.borrow()
    }
    fn row_mut(&self) -> std::cell::RefMut<'_, Row> {
        self.imp().row.borrow_mut()
    }
}

// ---------- Columns ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Name,
    Pid,
    User,
    Cpu,
    Mem,
    Disk,
    Gpu,
    Threads,
    State,
}

const COLUMNS: &[(Col, &str, &str, i32)] = &[
    (Col::Name, "name", "Name", 0),
    (Col::Pid, "pid", "PID", 66),
    (Col::User, "user", "User", 84),
    (Col::Cpu, "cpu", "CPU", 66),
    (Col::Mem, "mem", "Memory", 88),
    (Col::Disk, "disk", "Disk", 96),
    (Col::Gpu, "gpu", "GPU", 62),
    (Col::Threads, "threads", "Threads", 72),
    (Col::State, "state", "State", 88),
];

fn state_name(c: char) -> &'static str {
    match c {
        'R' => "Running",
        'S' => "Sleeping",
        'D' => "Waiting on disk",
        'T' => "Stopped",
        't' => "Traced",
        'Z' => "Zombie",
        'I' => "Idle",
        'X' => "Dead",
        _ => "Unknown",
    }
}

fn compare(col: Col, a: &Row, b: &Row) -> Ordering {
    match col {
        Col::Name => a.p.name.to_lowercase().cmp(&b.p.name.to_lowercase()).then(a.p.pid.cmp(&b.p.pid)),
        Col::Pid => a.p.pid.cmp(&b.p.pid),
        Col::User => a.p.user.cmp(&b.p.user).then(a.p.pid.cmp(&b.p.pid)),
        Col::Cpu => a.cpu().total_cmp(&b.cpu()),
        Col::Mem => a.mem().cmp(&b.mem()),
        Col::Disk => a.disk().total_cmp(&b.disk()),
        Col::Gpu => a.gpu().total_cmp(&b.gpu()),
        Col::Threads => a.threads().cmp(&b.threads()),
        Col::State => a.p.state.cmp(&b.p.state),
    }
}

// ---------- Page state ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Apps,
    All,
    Tree,
    Mine,
}

impl View {
    fn id(self) -> &'static str {
        match self {
            View::Apps => "apps",
            View::All => "all",
            View::Tree => "tree",
            View::Mine => "mine",
        }
    }
    fn from_id(s: &str) -> View {
        match s {
            "apps" => View::Apps,
            "tree" => View::Tree,
            "mine" => View::Mine,
            _ => View::All,
        }
    }
}

struct State {
    store: gio::ListStore,
    objects: HashMap<i32, ProcObject>,
    filter: gtk::CustomFilter,
    sorter: gtk::CustomSorter,
    selection: gtk::MultiSelection,
    view_widget: gtk::ColumnView,
    scroll: gtk::ScrolledWindow,
    columns: Vec<(Col, gtk::ColumnViewColumn)>,
    view: View,
    query: String,
    /// Apps view: groups the user opened. Tree view: branches the user closed.
    opened: HashSet<i32>,
    closed: HashSet<i32>,
    bound: HashMap<usize, (ProcObject, Col, gtk::Widget)>,
    icons: HashMap<(i32, String), String>,
    search: gtk::SearchEntry,
    chips: Vec<(View, gtk::Button)>,
    pending_reveal: Option<i32>,
    details: Option<Details>,
    my_uid: u32,
}

thread_local! {
    static STATE: RefCell<Option<Rc<RefCell<State>>>> = const { RefCell::new(None) };
}

fn state() -> Option<Rc<RefCell<State>>> {
    STATE.with(|s| s.borrow().clone())
}

// ---------- Cells ----------

fn num_label() -> gtk::Label {
    let l = gtk::Label::new(None);
    l.add_css_class("cell-num");
    l.set_xalign(1.0);
    l
}

fn setup_cell(col: Col) -> gtk::Widget {
    match col {
        Col::Name => {
            let b = widgets::hbox(6);
            let indent = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let expander = gtk::Button::from_icon_name("pan-end-symbolic");
            expander.add_css_class("tree-expander");
            expander.set_valign(gtk::Align::Center);
            expander.set_focusable(false);
            let icon = gtk::Image::new();
            icon.set_pixel_size(16);
            let name = widgets::label("", "");
            name.set_ellipsize(pango::EllipsizeMode::End);
            name.set_hexpand(true);
            let tag = widgets::tag("", "");
            b.append(&indent);
            b.append(&expander);
            b.append(&icon);
            b.append(&name);
            b.append(&tag);
            b.upcast()
        }
        Col::User | Col::State => {
            let l = widgets::label("", "cell-dim");
            l.set_ellipsize(pango::EllipsizeMode::End);
            l.upcast()
        }
        _ => num_label().upcast(),
    }
}

fn set_hot(l: &gtk::Label, hot: bool, zero: bool) {
    if hot {
        l.add_css_class("cell-hot");
    } else {
        l.remove_css_class("cell-hot");
    }
    if zero {
        l.add_css_class("cell-dim");
    } else {
        l.remove_css_class("cell-dim");
    }
}

fn render(col: Col, row: &Row, w: &gtk::Widget, view: View) {
    let tree = matches!(view, View::Apps | View::Tree);
    match col {
        Col::Name => {
            let Some(indent) = w.first_child() else { return };
            let Some(expander) = indent.next_sibling() else { return };
            let Some(icon) = expander.next_sibling().and_downcast::<gtk::Image>() else { return };
            let Some(name) = icon.next_sibling().and_downcast::<gtk::Label>() else { return };
            let Some(tag) = name.next_sibling().and_downcast::<gtk::Label>() else { return };
            indent.set_size_request((row.depth * 18) as i32, -1);
            expander.set_visible(tree && row.has_children);
            expander.set_opacity(if tree && row.has_children { 1.0 } else { 0.0 });
            if let Some(b) = expander.downcast_ref::<gtk::Button>() {
                b.set_icon_name(if row.expanded { "pan-down-symbolic" } else { "pan-end-symbolic" });
            }
            if row.icon.is_empty() {
                icon.clear();
            } else {
                icon.set_icon_name(Some(&row.icon));
            }
            let text = if view == View::Apps && row.depth == 0 && !row.title.is_empty() {
                row.title.clone()
            } else {
                row.p.name.clone()
            };
            if name.text() != text {
                name.set_text(&text);
            }
            let tip = if row.p.cmdline.is_empty() { format!("[{}]", row.p.name) } else { row.p.cmdline.clone() };
            name.set_tooltip_text(Some(&tip));
            match (row.agg, row.p.state) {
                (Some(a), _) if a.count > 1 => {
                    tag.set_text(&a.count.to_string());
                    tag.set_tooltip_text(Some(&format!("{} processes", a.count)));
                    tag.remove_css_class("danger");
                    tag.set_visible(true);
                }
                (_, 'T') => {
                    tag.set_text("Paused");
                    tag.set_tooltip_text(None);
                    tag.add_css_class("danger");
                    tag.set_visible(true);
                }
                (_, 'Z') => {
                    tag.set_text("Zombie");
                    tag.set_tooltip_text(None);
                    tag.add_css_class("danger");
                    tag.set_visible(true);
                }
                _ => tag.set_visible(false),
            }
        }
        _ => {
            let Some(l) = w.downcast_ref::<gtk::Label>() else { return };
            let (text, hot, zero) = match col {
                Col::Pid => (row.p.pid.to_string(), false, false),
                Col::User => (row.p.user.clone(), false, false),
                Col::Cpu => {
                    let v = row.cpu();
                    (if v < 0.05 { "0".into() } else { format!("{v:.1}") }, v >= 25.0, v < 0.05)
                }
                Col::Mem => (fmt::bytes(row.mem() as f64), row.mem() > 1 << 31, row.mem() == 0),
                Col::Disk => {
                    let v = row.disk();
                    (if v < 1.0 { "0".into() } else { fmt::rate(v) }, v > 20.0 * 1024.0 * 1024.0, v < 1.0)
                }
                Col::Gpu => {
                    let v = row.gpu();
                    if v >= 0.05 {
                        (format!("{v:.1}"), v >= 25.0, false)
                    } else if row.p.nvidia {
                        ("dGPU".into(), false, false)
                    } else {
                        ("0".into(), false, true)
                    }
                }
                Col::Threads => (row.threads().to_string(), false, false),
                Col::State => (state_name(row.p.state).to_string(), false, false),
                Col::Name => unreachable!(),
            };
            if l.text() != text {
                l.set_text(&text);
            }
            if !matches!(col, Col::User | Col::State) {
                set_hot(l, hot, zero);
            }
        }
    }
}

fn key(w: &gtk::Widget) -> usize {
    w.as_ptr() as usize
}

fn make_column(col: Col, title: &str, width: i32) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let w = setup_cell(col);
        if col == Col::Name
            && let Some(expander) = w.first_child().and_then(|i| i.next_sibling()).and_downcast::<gtk::Button>()
        {
            let item = item.downgrade();
            expander.connect_clicked(move |_| {
                if let Some(obj) = item.upgrade().and_then(|i| i.item()).and_downcast::<ProcObject>() {
                    toggle_expanded(obj.row().p.pid);
                }
            });
        }
        item.set_child(Some(&w));
    });
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let (Some(obj), Some(w)) = (item.item().and_downcast::<ProcObject>(), item.child()) else { return };
        let Some(st) = state() else { return };
        let view = st.borrow().view;
        render(col, &obj.row(), &w, view);
        st.borrow_mut().bound.insert(key(&w), (obj, col, w));
    });
    factory.connect_unbind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        if let (Some(w), Some(st)) = (item.child(), state()) {
            st.borrow_mut().bound.remove(&key(&w));
        }
    });
    let c = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    // A sorter that never reorders; it only gives the header its sort indicator.
    // The real order is worked out in `update`.
    c.set_sorter(Some(&gtk::CustomSorter::new(|_, _| gtk::Ordering::Equal)));
    c.set_resizable(true);
    if width > 0 {
        c.set_fixed_width(width);
    } else {
        c.set_expand(true);
    }
    c
}

// ---------- Update ----------

fn sort_state(st: &State) -> (Col, bool) {
    let Some(cvs) = st.view_widget.sorter().and_downcast::<gtk::ColumnViewSorter>() else { return (Col::Cpu, true) };
    let Some(primary) = cvs.primary_sort_column() else { return (Col::Cpu, true) };
    let col = st.columns.iter().find(|(_, c)| c == &primary).map(|(c, _)| *c).unwrap_or(Col::Cpu);
    (col, cvs.primary_sort_order() == gtk::SortType::Descending)
}

fn matches(row: &Row, q: &str) -> bool {
    q.is_empty()
        || row.p.name.to_lowercase().contains(q)
        || row.p.cmdline.to_lowercase().contains(q)
        || row.p.pid.to_string() == q
        || row.p.user.to_lowercase() == q
        || row.title.to_lowercase().contains(q)
}

fn icon_for(p: &Proc, class: Option<&str>) -> String {
    let found = class.and_then(apps::lookup).or_else(|| apps::lookup(&p.name));
    match found {
        Some(a) if !a.icon.is_empty() => a.icon,
        // Background processes get no icon; the column stays aligned.
        _ => String::new(),
    }
}

/// Take in a snapshot: sync objects, then recompute the view.
///
/// GTK re-binds rows synchronously while the store, filter or sorter change,
/// and binding reads the page state, so those calls happen with no borrow held.
fn update(snap: &Snapshot) {
    let Some(st) = state() else { return };
    let Some(procs) = &snap.procs else { return };
    let windows: HashMap<i32, &crate::sampler::windows::Window> = snap.windows.iter().map(|w| (w.pid, w)).collect();
    let window_pids: HashSet<i32> = windows.keys().copied().collect();
    let (store, removed, added) = {
        let mut s = st.borrow_mut();
        let alive: HashSet<i32> = procs.iter().map(|p| p.pid).collect();
        let dead: Vec<i32> = s.objects.keys().filter(|p| !alive.contains(p)).copied().collect();
        let removed: Vec<ProcObject> = dead.iter().filter_map(|pid| s.objects.remove(pid)).collect();
        let mut added = Vec::new();
        for p in procs {
            let win = windows.get(&p.pid);
            let class = win.map(|w| w.class.clone());
            let icon_key = (p.pid, class.clone().unwrap_or_default());
            let icon = match s.icons.get(&icon_key) {
                Some(i) => i.clone(),
                None => {
                    let i = icon_for(p, class.as_deref());
                    s.icons.insert(icon_key, i.clone());
                    i
                }
            };
            let title = win.map(|w| w.title.clone()).unwrap_or_default();
            let address = win.map(|w| w.address.clone()).unwrap_or_default();
            if let Some(obj) = s.objects.get(&p.pid) {
                let mut r = obj.row_mut();
                r.p = p.clone();
                r.icon = icon;
                r.title = title;
                r.address = address;
            } else {
                let obj = ProcObject::new(Row { p: p.clone(), icon, title, address, ..Default::default() });
                added.push(obj.clone());
                s.objects.insert(p.pid, obj);
            }
        }
        s.icons.retain(|(pid, _), _| alive.contains(pid));
        layout(&mut s, &window_pids);
        (s.store.clone(), removed, added)
    };
    for obj in removed {
        if let Some(pos) = store.find(&obj) {
            store.remove(pos);
        }
    }
    if !added.is_empty() {
        store.extend_from_slice(&added);
    }
    apply_view(&st);
    if let Some(pid) = st.borrow_mut().pending_reveal.take() {
        select_pid(pid);
    }
    refresh_details(snap);
}

/// Re-run the filter and sort, then refresh the cells on screen.
///
/// ListView keeps its top row anchored, so after a re-sort it would scroll to
/// wherever that row moved. We hold the scroll position still instead.
fn apply_view(st: &Rc<RefCell<State>>) {
    let (filter, sorter, cv, adj) = {
        let s = st.borrow();
        (s.filter.clone(), s.sorter.clone(), s.view_widget.clone(), s.scroll.vadjustment())
    };
    let before = adj.value();
    filter.changed(gtk::FilterChange::Different);
    sorter.changed(gtk::SorterChange::Different);
    if before < 1.0 {
        if cv.model().is_some_and(|m| m.n_items() > 0) {
            cv.scroll_to(0, None, gtk::ListScrollFlags::NONE, None);
        }
    } else {
        adj.set_value(before);
        glib::idle_add_local_once(move || adj.set_value(before));
    }
    let (view, bound): (View, Vec<(ProcObject, Col, gtk::Widget)>) = {
        let s = st.borrow();
        (s.view, s.bound.values().cloned().collect())
    };
    for (obj, col, w) in bound {
        render(col, &obj.row(), &w, view);
    }
}

/// Per pid: visible, order, depth, has children, expanded, collapsed totals.
type Placement = (bool, u32, u32, bool, bool, Option<Agg>);

/// Work out visibility, order, depth and aggregates for the current view.
fn layout(s: &mut State, window_pids: &HashSet<i32>) {
    let (col, desc) = sort_state(s);
    let q = s.query.to_lowercase();
    let q = q.trim();
    let show_kernel = prefs::get().kernel_threads;
    let rows: HashMap<i32, Row> = s.objects.iter().map(|(pid, o)| (*pid, o.row().clone())).collect();
    let cmp = |a: &i32, b: &i32| {
        let o = compare(col, &rows[a], &rows[b]);
        if desc { o.reverse() } else { o }
    };
    let mut out: HashMap<i32, Placement> = HashMap::new();

    let flat = |filter: &dyn Fn(&Row) -> bool, out: &mut HashMap<i32, Placement>| {
        let mut list: Vec<i32> = rows.values().filter(|r| filter(r)).map(|r| r.p.pid).collect();
        list.sort_by(cmp);
        for (i, pid) in list.iter().enumerate() {
            out.insert(*pid, (true, i as u32, 0, false, false, None));
        }
    };

    let tree_view = matches!(s.view, View::Apps | View::Tree) && q.is_empty();
    if !tree_view {
        let my_uid = s.my_uid;
        let mine = s.view == View::Mine;
        let apps = s.view == View::Apps;
        flat(
            &|r: &Row| {
                (show_kernel || !r.p.kernel || mine)
                    && (!mine || (r.p.uid == my_uid && !r.p.kernel))
                    && (!apps || window_pids.contains(&r.p.pid))
                    && matches(r, q)
            },
            &mut out,
        );
    } else {
        let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
        for r in rows.values() {
            if (!show_kernel && r.p.kernel && s.view == View::Tree) || r.p.pid == r.p.ppid {
                continue;
            }
            children.entry(r.p.ppid).or_default().push(r.p.pid);
        }
        for list in children.values_mut() {
            list.sort_by(cmp);
        }
        // Subtree sums.
        fn agg(pid: i32, rows: &HashMap<i32, Row>, children: &HashMap<i32, Vec<i32>>, memo: &mut HashMap<i32, Agg>) -> Agg {
            if let Some(a) = memo.get(&pid) {
                return *a;
            }
            let r = &rows[&pid];
            let mut a = Agg {
                cpu: r.p.cpu,
                mem: r.p.rss,
                disk: r.p.read_bps + r.p.write_bps,
                gpu: r.p.gpu,
                threads: r.p.threads,
                count: 1,
            };
            for c in children.get(&pid).into_iter().flatten() {
                let ca = agg(*c, rows, children, memo);
                a.cpu += ca.cpu;
                a.mem += ca.mem;
                a.disk += ca.disk;
                a.gpu = (a.gpu + ca.gpu).min(100.0);
                a.threads += ca.threads;
                a.count += ca.count;
            }
            memo.insert(pid, a);
            a
        }
        let mut memo = HashMap::new();
        let mut roots: Vec<i32> = if s.view == View::Apps {
            // Outermost window owners only (a terminal's child windows fold into it).
            window_pids
                .iter()
                .filter(|pid| rows.contains_key(pid))
                .filter(|pid| {
                    let mut cur = rows[pid].p.ppid;
                    let mut hops = 0;
                    while let Some(r) = rows.get(&cur) {
                        if window_pids.contains(&cur) {
                            return false;
                        }
                        cur = r.p.ppid;
                        hops += 1;
                        if hops > 64 {
                            break;
                        }
                    }
                    true
                })
                .copied()
                .collect()
        } else {
            rows.values()
                .filter(|r| !rows.contains_key(&r.p.ppid) || r.p.ppid == r.p.pid)
                .filter(|r| show_kernel || !r.p.kernel)
                .map(|r| r.p.pid)
                .collect()
        };
        // Sort roots by their (aggregated, for apps) value.
        let apps = s.view == View::Apps;
        roots.sort_by(|a, b| {
            let o = if apps && matches!(col, Col::Cpu | Col::Mem | Col::Disk | Col::Gpu | Col::Threads) {
                let (x, y) = (agg(*a, &rows, &children, &mut memo), agg(*b, &rows, &children, &mut memo));
                match col {
                    Col::Cpu => x.cpu.total_cmp(&y.cpu),
                    Col::Mem => x.mem.cmp(&y.mem),
                    Col::Disk => x.disk.total_cmp(&y.disk),
                    Col::Gpu => x.gpu.total_cmp(&y.gpu),
                    _ => x.threads.cmp(&y.threads),
                }
            } else {
                compare(col, &rows[a], &rows[b])
            };
            if desc { o.reverse() } else { o }
        });
        let mut order = 0u32;
        let mut stack: Vec<(i32, u32, bool)> = roots.iter().rev().map(|p| (*p, 0, true)).collect();
        while let Some((pid, depth, shown)) = stack.pop() {
            let kids = children.get(&pid).cloned().unwrap_or_default();
            let has_children = !kids.is_empty();
            let expanded = has_children && if s.view == View::Apps { s.opened.contains(&pid) } else { !s.closed.contains(&pid) };
            let a = if has_children && !expanded { Some(agg(pid, &rows, &children, &mut memo)) } else { None };
            out.insert(pid, (shown, order, depth, has_children, expanded, a));
            order += 1;
            for k in kids.iter().rev() {
                stack.push((*k, depth + 1, shown && expanded));
            }
        }
    }
    for (pid, obj) in &s.objects {
        let mut r = obj.row_mut();
        match out.get(pid) {
            Some(&(visible, order, depth, has_children, expanded, agg)) => {
                r.visible = visible;
                r.order = order;
                r.depth = depth;
                r.has_children = has_children;
                r.expanded = expanded;
                r.agg = agg;
            }
            None => {
                r.visible = false;
                r.order = u32::MAX;
                r.depth = 0;
                r.has_children = false;
                r.agg = None;
            }
        }
    }
}

/// Recompute the view right away (after a view, filter or expand change).
pub fn refilter() {
    let Some(st) = state() else { return };
    if let Some(snap) = live::latest() {
        update(&snap);
    } else {
        apply_view(&st);
    }
}

fn toggle_expanded(pid: i32) {
    let Some(st) = state() else { return };
    {
        let mut s = st.borrow_mut();
        let set = if s.view == View::Apps { &mut s.opened } else { &mut s.closed };
        if !set.remove(&pid) {
            set.insert(pid);
        }
    }
    refilter();
}

fn set_view(v: View) {
    let Some(st) = state() else { return };
    {
        let mut s = st.borrow_mut();
        s.view = v;
        for (view, b) in &s.chips {
            if *view == v {
                b.add_css_class("selected");
            } else {
                b.remove_css_class("selected");
            }
        }
    }
    prefs::update(|p| p.process_view = v.id().into());
    refilter();
}

fn selected(st: &State) -> Vec<ProcObject> {
    let sel = st.selection.selection();
    let mut out = Vec::new();
    if let Some((iter, first)) = gtk::BitsetIter::init_first(&sel) {
        for pos in std::iter::once(first).chain(iter) {
            if let Some(o) = st.selection.item(pos).and_downcast::<ProcObject>() {
                out.push(o);
            }
        }
    }
    out
}

fn select_pid(pid: i32) {
    let Some(st) = state() else { return };
    let s = st.borrow();
    let n = s.selection.n_items();
    for i in 0..n {
        if let Some(o) = s.selection.item(i).and_downcast::<ProcObject>()
            && o.row().p.pid == pid
        {
            s.selection.select_item(i, true);
            let cv = s.view_widget.clone();
            drop(s);
            cv.scroll_to(i, None, gtk::ListScrollFlags::FOCUS, None);
            return;
        }
    }
}

/// Show one process in the table (from the Overview).
pub fn reveal(pid: i32) {
    window::navigate("processes");
    let Some(st) = state() else { return };
    {
        let mut s = st.borrow_mut();
        s.query.clear();
        s.search.set_text("");
        if s.view == View::Apps {
            drop(s);
            set_view(View::All);
        }
    }
    let visible = st.borrow().objects.get(&pid).is_some_and(|o| o.row().visible);
    if visible {
        select_pid(pid);
    } else {
        st.borrow_mut().pending_reveal = Some(pid);
    }
}

pub fn focus_search() {
    if let Some(st) = state() {
        let e = st.borrow().search.clone();
        e.grab_focus();
    }
}

fn targets(st: &State) -> Vec<(i32, String)> {
    selected(st).iter().map(|o| (o.row().p.pid, o.row().p.name.clone())).collect()
}

fn end_selected(sig: Signal) {
    let Some(st) = state() else { return };
    let t = targets(&st.borrow());
    if !t.is_empty() {
        actions::signal(&t, sig);
    }
}

// ---------- Details ----------

struct Details {
    revealer: gtk::Revealer,
    icon: gtk::Image,
    title: gtk::Label,
    subtitle: gtk::Label,
    values: Vec<gtk::Label>,
    cmdline: gtk::Label,
    graphs: gtk::Box,
    pid: Rc<Cell<i32>>,
    focus: gtk::Button,
    close: gtk::Button,
    pause: gtk::Button,
    end: gtk::Button,
}

fn details_panel() -> Details {
    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
    revealer.set_transition_duration(if prefs::get().reduce_motion { 0 } else { 160 });
    let panel = widgets::vbox(10);
    panel.add_css_class("details-panel");
    revealer.set_child(Some(&panel));
    let pid = Rc::new(Cell::new(0));

    let head = widgets::hbox(10);
    let icon = gtk::Image::new();
    icon.set_pixel_size(28);
    let text = widgets::vbox(0);
    text.set_hexpand(true);
    let title = widgets::label("", "details-title");
    title.set_ellipsize(pango::EllipsizeMode::End);
    let subtitle = widgets::label("", "dim");
    subtitle.add_css_class("mono");
    subtitle.set_ellipsize(pango::EllipsizeMode::End);
    text.append(&title);
    text.append(&subtitle);
    head.append(&icon);
    head.append(&text);
    let hide = gtk::Button::from_icon_name("window-close-symbolic");
    hide.add_css_class("flat");
    hide.set_tooltip_text(Some("Hide details"));
    hide.set_valign(gtk::Align::Start);
    hide.connect_clicked(|_| {
        if let Some(st) = state() {
            st.borrow().selection.unselect_all();
        }
    });
    head.append(&hide);
    panel.append(&head);

    let (flow, values) =
        widgets::kv_flow(&["CPU", "Memory", "Disk read", "Disk write", "Threads", "Priority", "Open files", "Started"]);
    flow.set_max_children_per_line(8);
    panel.append(&flow);
    let graphs = widgets::hbox(10);
    graphs.set_homogeneous(true);
    panel.append(&graphs);
    let cmdline = widgets::label("", "code-block");
    cmdline.set_wrap(true);
    cmdline.set_wrap_mode(pango::WrapMode::WordChar);
    cmdline.set_selectable(true);
    cmdline.set_lines(3);
    cmdline.set_ellipsize(pango::EllipsizeMode::End);
    panel.append(&cmdline);

    // Actions.
    let buttons = gtk::FlowBox::new();
    buttons.set_selection_mode(gtk::SelectionMode::None);
    buttons.set_row_spacing(6);
    buttons.set_column_spacing(6);
    buttons.set_max_children_per_line(8);
    let p = pid.clone();
    let focus = gtk::Button::with_label("Switch to");
    focus.connect_clicked(move |_| {
        if let Some(o) = object(p.get()) {
            actions::focus_window(&o.row().address);
        }
    });
    let p = pid.clone();
    let close = gtk::Button::with_label("Close window");
    close.connect_clicked(move |_| {
        if let Some(o) = object(p.get()) {
            actions::close_window(&o.row().address);
        }
    });
    let p = pid.clone();
    let pause = gtk::Button::with_label("Pause");
    pause.set_tooltip_text(Some("Freeze the process (SIGSTOP) until you resume it"));
    pause.connect_clicked(move |_| {
        if let Some(o) = object(p.get()) {
            let r = o.row();
            let sig = if r.p.state == 'T' { Signal::Cont } else { Signal::Stop };
            actions::signal(&[(r.p.pid, r.p.name.clone())], sig);
        }
    });
    let priority = gtk::MenuButton::new();
    priority.set_label("Priority");
    let pop = gtk::Popover::new();
    let plist = widgets::vbox(2);
    for (label, nice) in [
        ("Highest (-15)", -15),
        ("High (-5)", -5),
        ("Normal (0)", 0),
        ("Low (5)", 5),
        ("Lowest (10)", 10),
        ("Background (19)", 19),
    ] {
        let b = gtk::Button::with_label(label);
        b.add_css_class("flat");
        let p = pid.clone();
        let pop2 = pop.clone();
        b.connect_clicked(move |_| {
            pop2.popdown();
            if let Some(o) = object(p.get()) {
                let r = o.row();
                actions::renice(r.p.pid, &r.p.name, nice);
            }
        });
        plist.append(&b);
    }
    pop.set_child(Some(&plist));
    priority.set_popover(Some(&pop));
    let p = pid.clone();
    let location = gtk::Button::with_label("Open location");
    location.connect_clicked(move |_| actions::open_location(p.get()));
    let p = pid.clone();
    let copy = gtk::Button::with_label("Copy command");
    copy.connect_clicked(move |_| {
        if let Some(o) = object(p.get()) {
            actions::copy(&o.row().p.cmdline);
        }
    });
    let p = pid.clone();
    let kill = kill_button("Kill", "Click again to kill", Signal::Kill, p);
    kill.set_tooltip_text(Some("Stop it immediately (SIGKILL). Unsaved work is lost."));
    let p = pid.clone();
    let end = kill_button("End task", "Click again to end", Signal::Term, p);
    end.set_tooltip_text(Some("Ask it to quit (SIGTERM)"));
    for w in [
        focus.upcast_ref::<gtk::Widget>(),
        close.upcast_ref(),
        pause.upcast_ref(),
        priority.upcast_ref(),
        location.upcast_ref(),
        copy.upcast_ref(),
        kill.upcast_ref(),
        end.upcast_ref(),
    ] {
        buttons.append(w);
    }
    let mut child = buttons.first_child();
    while let Some(c) = child {
        c.set_focusable(false);
        child = c.next_sibling();
    }
    panel.append(&buttons);

    Details { revealer, icon, title, subtitle, values, cmdline, graphs, pid, focus, close, pause, end }
}

/// End/Kill in the details panel: two clicks when "Ask before ending tasks" is on.
fn kill_button(label: &str, armed: &str, sig: Signal, pid: Rc<Cell<i32>>) -> gtk::Button {
    let act = move || {
        if let Some(o) = object(pid.get()) {
            let r = o.row();
            actions::signal(&[(r.p.pid, r.p.name.clone())], sig);
        }
    };
    if prefs::get().confirm_kill {
        widgets::two_click(label, armed, act)
    } else {
        let b = gtk::Button::with_label(label);
        b.add_css_class("destructive-action");
        b.connect_clicked(move |_| act());
        b
    }
}

fn object(pid: i32) -> Option<ProcObject> {
    state().and_then(|s| s.borrow().objects.get(&pid).cloned())
}

fn refresh_details(snap: &Snapshot) {
    let Some(st) = state() else { return };
    let s = st.borrow();
    let Some(d) = &s.details else { return };
    let sel = selected(&s);
    let Some(obj) = sel.first() else {
        d.revealer.set_reveal_child(false);
        return;
    };
    let r = obj.row().clone();
    d.revealer.set_reveal_child(true);
    if d.pid.get() != r.p.pid {
        d.pid.set(r.p.pid);
        live::track(r.p.pid);
        while let Some(c) = d.graphs.first_child() {
            d.graphs.remove(&c);
        }
        let cpu = graph::sparkline(&format!("proc.{}.cpu", r.p.pid), graph::Tone::Accent, graph::Scale::Auto { floor: 5.0 }, 40);
        let mem = graph::sparkline(&format!("proc.{}.mem", r.p.pid), graph::Tone::Second, graph::Scale::Auto { floor: 1e6 }, 40);
        for (title, g) in [("CPU", cpu), ("Memory", mem)] {
            let b = widgets::vbox(2);
            b.add_css_class("core-cell");
            b.append(&widgets::label(title, "core-name"));
            b.append(&g);
            d.graphs.append(&b);
        }
    }
    d.icon.set_icon_name(Some(if r.icon.is_empty() { "application-x-executable-symbolic" } else { &r.icon }));
    let name = if r.title.is_empty() { r.p.name.clone() } else { format!("{} — {}", r.p.name, r.title) };
    d.title.set_text(&name);
    let more = if sel.len() > 1 { format!(" · {} selected", sel.len()) } else { String::new() };
    d.subtitle.set_text(&format!("PID {} · parent {} · {} · {}{more}", r.p.pid, r.p.ppid, r.p.user, state_name(r.p.state)));
    let boot = glib::DateTime::now_local().ok().map(|n| n.to_unix() as f64 - snap.cpu.uptime);
    let started = boot
        .and_then(|b| glib::DateTime::from_unix_local((b + r.p.start) as i64).ok())
        .and_then(|t| t.format("%H:%M · %b %e").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "–".into());
    let vals = [
        fmt::pct(r.p.cpu),
        fmt::bytes(r.p.rss as f64),
        fmt::rate(r.p.read_bps),
        fmt::rate(r.p.write_bps),
        r.p.threads.to_string(),
        r.p.nice.to_string(),
        if r.p.fds > 0 { r.p.fds.to_string() } else { "–".into() },
        started,
    ];
    for (l, v) in d.values.iter().zip(vals) {
        l.set_text(&v);
    }
    let cmd = if r.p.cmdline.is_empty() { format!("[{}] (kernel thread)", r.p.name) } else { r.p.cmdline.clone() };
    if d.cmdline.text() != cmd {
        d.cmdline.set_text(&cmd);
    }
    let has_window = !r.address.is_empty();
    d.focus.set_visible(has_window);
    d.close.set_visible(has_window);
    d.pause.set_label(if r.p.state == 'T' { "Resume" } else { "Pause" });
    d.end.set_sensitive(!r.p.kernel);
}

// ---------- Build ----------

pub fn build(page: &Page) {
    let p = prefs::get();
    let view = View::from_id(&p.process_view);

    // Toolbar: views, filter, columns.
    let toolbar = widgets::hbox(8);
    toolbar.add_css_class("toolbar");
    let chips_box = widgets::hbox(6);
    let mut chips = Vec::new();
    for (v, label, tip) in [
        (View::Apps, "Apps", "Programs with windows, with their helper processes"),
        (View::All, "All", "Every process"),
        (View::Tree, "Tree", "Processes under the ones that started them"),
        (View::Mine, "Mine", "Only your own processes"),
    ] {
        let b = gtk::Button::with_label(label);
        b.add_css_class("chip");
        b.set_tooltip_text(Some(tip));
        if v == view {
            b.add_css_class("selected");
        }
        b.connect_clicked(move |_| set_view(v));
        chips_box.append(&b);
        chips.push((v, b));
    }
    toolbar.append(&chips_box);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Filter by name, command, PID or user"));
    search.set_hexpand(true);
    toolbar.append(&search);

    let columns_menu = gtk::MenuButton::new();
    columns_menu.set_icon_name("view-list-symbolic");
    columns_menu.set_tooltip_text(Some("Columns"));
    toolbar.append(&columns_menu);
    page.body.append(&toolbar);

    // Model: store -> filter -> sort -> selection.
    let store = gio::ListStore::new::<ProcObject>();
    let filter = gtk::CustomFilter::new(|o| o.downcast_ref::<ProcObject>().is_some_and(|o| o.row().visible));
    let filtered = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
    let sorter = gtk::CustomSorter::new(|a, b| {
        let (Some(a), Some(b)) = (a.downcast_ref::<ProcObject>(), b.downcast_ref::<ProcObject>()) else {
            return gtk::Ordering::Equal;
        };
        a.row().order.cmp(&b.row().order).into()
    });
    let sorted = gtk::SortListModel::new(Some(filtered), Some(sorter.clone()));
    let selection = gtk::MultiSelection::new(Some(sorted));
    let cv = gtk::ColumnView::new(Some(selection.clone()));
    cv.set_show_column_separators(false);
    cv.set_reorderable(true);
    cv.set_hexpand(true);
    cv.set_vexpand(true);

    let hidden = p.hidden_columns.clone();
    let mut columns = Vec::new();
    let colbox = widgets::vbox(2);
    for (col, id, title, width) in COLUMNS {
        let c = make_column(*col, title, *width);
        c.set_id(Some(id));
        c.set_visible(!hidden.iter().any(|h| h == id));
        cv.append_column(&c);
        if *col != Col::Name {
            let check = gtk::CheckButton::with_label(title);
            check.set_active(c.is_visible());
            let c2 = c.clone();
            let id = id.to_string();
            check.connect_toggled(move |b| {
                c2.set_visible(b.is_active());
                let on = b.is_active();
                let id = id.clone();
                prefs::update(move |p| {
                    p.hidden_columns.retain(|h| *h != id);
                    if !on {
                        p.hidden_columns.push(id);
                    }
                });
            });
            colbox.append(&check);
        }
        columns.push((*col, c));
    }
    let pop = gtk::Popover::new();
    pop.set_child(Some(&colbox));
    columns_menu.set_popover(Some(&pop));
    if let Some(cpu) = columns.iter().find(|(c, _)| *c == Col::Cpu).map(|(_, c)| c.clone()) {
        cv.sort_by_column(Some(&cpu), gtk::SortType::Descending);
    }
    if let Some(cvs) = cv.sorter() {
        cvs.connect_changed(|_, _| refilter());
    }

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .child(&cv)
        .vexpand(true)
        .build();
    let card = widgets::vbox(0);
    card.add_css_class("table-card");
    card.set_overflow(gtk::Overflow::Hidden);
    card.append(&scroll);
    card.set_vexpand(true);
    page.body.append(&card);

    let details = details_panel();
    page.body.append(&details.revealer);

    let my_uid = unsafe { libc::getuid() };
    let st = State {
        store,
        objects: HashMap::new(),
        filter,
        sorter,
        selection: selection.clone(),
        view_widget: cv.clone(),
        scroll: scroll.clone(),
        columns,
        view,
        query: String::new(),
        opened: HashSet::new(),
        closed: HashSet::new(),
        bound: HashMap::new(),
        icons: HashMap::new(),
        search: search.clone(),
        chips,
        pending_reveal: None,
        details: Some(details),
        my_uid,
    };
    STATE.with(|s| *s.borrow_mut() = Some(Rc::new(RefCell::new(st))));

    search.connect_search_changed(|e| {
        if let Some(st) = state() {
            st.borrow_mut().query = e.text().to_string();
        }
        refilter();
    });
    selection.connect_selection_changed(|_, _, _| {
        if let Some(snap) = live::latest() {
            refresh_details(&snap);
        }
    });
    cv.connect_activate(|cv, pos| {
        let Some(o) = cv.model().and_then(|m| m.item(pos)).and_downcast::<ProcObject>() else { return };
        let (pid, has_children, address) = {
            let r = o.row();
            (r.p.pid, r.has_children, r.address.clone())
        };
        if has_children {
            toggle_expanded(pid);
        } else if !address.is_empty() {
            actions::focus_window(&address);
        }
    });

    // Delete ends, Shift+Delete kills. With confirmation on, press twice.
    let armed: Rc<Cell<Option<(Signal, std::time::Instant)>>> = Rc::new(Cell::new(None));
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(move |_, key, _, mods| {
        if key != gdk::Key::Delete && key != gdk::Key::KP_Delete {
            return glib::Propagation::Proceed;
        }
        let sig = if mods.contains(gdk::ModifierType::SHIFT_MASK) { Signal::Kill } else { Signal::Term };
        let Some(st) = state() else { return glib::Propagation::Stop };
        let t = targets(&st.borrow());
        if t.is_empty() {
            return glib::Propagation::Stop;
        }
        let confirm = prefs::get().confirm_kill;
        let ready = matches!(armed.get(), Some((s, at)) if s == sig && at.elapsed().as_secs_f64() < 3.0);
        if !confirm || ready {
            armed.set(None);
            end_selected(sig);
        } else {
            armed.set(Some((sig, std::time::Instant::now())));
            let what = if t.len() == 1 { t[0].1.clone() } else { format!("{} processes", t.len()) };
            let verb = if sig == Signal::Kill { "kill" } else { "end" };
            window::toast(&format!("Press again to {verb} {what}"));
        }
        glib::Propagation::Stop
    });
    cv.add_controller(keys);

    live::on_tick(&cv, update);
}
