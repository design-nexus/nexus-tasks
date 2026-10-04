use crate::graph::{self, Scale, Series, Tone};
use crate::sampler::Snapshot;
use crate::sampler::procs::Proc;
use crate::widgets::{self, Page};
use crate::{fmt, live, prefs, window};
use gtk::pango;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Every tile the overview can show, in the default order, with its name.
pub const TILES: &[(&str, &str)] = &[
    ("cpu", "CPU"),
    ("mem", "Memory"),
    ("gpu", "GPU"),
    ("disk", "Disk"),
    ("net", "Network"),
    ("temp", "Temperature"),
    ("bat", "Battery"),
];

struct Tile {
    button: gtk::Button,
    value: gtk::Label,
    sub: gtk::Label,
}

fn tile(title: &str, target: &'static str, spark: gtk::DrawingArea) -> Tile {
    let b = gtk::Button::new();
    b.add_css_class("stat-tile");
    let v = widgets::vbox(2);
    v.append(&widgets::label(&title.to_uppercase(), "stat-title"));
    let value = widgets::label("–", "stat-value");
    value.add_css_class("mono");
    v.append(&value);
    let sub = widgets::label("", "stat-sub");
    sub.set_ellipsize(pango::EllipsizeMode::End);
    v.append(&sub);
    spark.set_margin_top(6);
    v.append(&spark);
    b.set_child(Some(&v));
    b.set_tooltip_text(Some(&format!("Open {title}")));
    b.connect_clicked(move |_| window::navigate(target));
    Tile { button: b, value, sub }
}

/// Put the chosen tiles into the flow box, in the chosen order.
fn arrange(flow: &gtk::FlowBox, tiles: &[(&'static str, gtk::Button)]) {
    while let Some(c) = flow.first_child() {
        if let Some(child) = c.downcast_ref::<gtk::FlowBoxChild>() {
            child.set_child(None::<&gtk::Widget>);
        }
        flow.remove(&c);
    }
    for id in prefs::get().overview_tiles {
        if let Some((_, b)) = tiles.iter().find(|(t, _)| *t == id) {
            flow.append(b);
            if let Some(child) = flow.last_child() {
                child.set_focusable(false);
            }
        }
    }
}

/// The tile picker: a check to show each tile, and arrows to move it.
fn tile_editor(flow: &gtk::FlowBox, tiles: Rc<Vec<(&'static str, gtk::Button)>>) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_icon_name("document-edit-symbolic");
    button.add_css_class("flat");
    button.add_css_class("group-action");
    button.set_tooltip_text(Some("Choose and order tiles"));
    let pop = gtk::Popover::new();
    let list = widgets::vbox(2);
    list.add_css_class("tile-editor");
    pop.set_child(Some(&list));
    button.set_popover(Some(&pop));

    // Rebuilt after each change, since the order changes.
    fn fill(list: &gtk::Box, flow: &gtk::FlowBox, tiles: &Rc<Vec<(&'static str, gtk::Button)>>) {
        while let Some(c) = list.first_child() {
            list.remove(&c);
        }
        let shown = prefs::get().overview_tiles;
        // Shown tiles in order, then the hidden ones.
        let mut ids: Vec<&'static str> =
            shown.iter().filter_map(|s| tiles.iter().find(|(t, _)| t == s).map(|(t, _)| *t)).collect();
        let hidden: Vec<&'static str> = tiles.iter().map(|(t, _)| *t).filter(|t| !ids.contains(t)).collect();
        ids.extend(hidden);
        for (i, id) in ids.iter().enumerate() {
            let name = TILES.iter().find(|(t, _)| t == id).map_or(*id, |(_, n)| *n);
            let row = widgets::hbox(4);
            let check = gtk::CheckButton::with_label(name);
            check.set_active(shown.iter().any(|s| s == id));
            check.set_hexpand(true);
            row.append(&check);
            let up = gtk::Button::from_icon_name("go-up-symbolic");
            let down = gtk::Button::from_icon_name("go-down-symbolic");
            for (b, tip) in [(&up, "Move up"), (&down, "Move down")] {
                b.add_css_class("flat");
                b.set_tooltip_text(Some(tip));
                row.append(b);
            }
            up.set_sensitive(i > 0);
            down.set_sensitive(i + 1 < ids.len());
            let apply = {
                let (list, flow, tiles) = (list.clone(), flow.clone(), tiles.clone());
                move |order: Vec<&'static str>, on: Vec<&'static str>| {
                    prefs::update(|p| {
                        p.overview_tiles = order.iter().filter(|t| on.contains(t)).map(|t| t.to_string()).collect();
                    });
                    arrange(&flow, &tiles);
                    let (list, flow, tiles) = (list.clone(), flow.clone(), tiles.clone());
                    // Not from inside the handler of a button that's about to go.
                    gtk::glib::idle_add_local_once(move || fill(&list, &flow, &tiles));
                }
            };
            let enabled = {
                let shown = shown.clone();
                move || -> Vec<&'static str> { TILES.iter().map(|(t, _)| *t).filter(|t| shown.iter().any(|s| s == t)).collect() }
            };
            let (a, ids2, id, en) = (apply.clone(), ids.clone(), *id, enabled.clone());
            check.connect_toggled(move |c| {
                let mut on = en();
                on.retain(|t| *t != id);
                if c.is_active() {
                    on.push(id);
                }
                a(ids2.clone(), on);
            });
            let (a, ids2, en) = (apply.clone(), ids.clone(), enabled.clone());
            up.connect_clicked(move |_| {
                let mut order = ids2.clone();
                order.swap(i, i - 1);
                a(order, en());
            });
            let (a, ids2, en) = (apply, ids.clone(), enabled);
            down.connect_clicked(move |_| {
                let mut order = ids2.clone();
                order.swap(i, i + 1);
                a(order, en());
            });
            list.append(&row);
        }
    }
    fill(&list, flow, &tiles);
    button
}

// ---------- Busiest processes ----------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Metric {
    Cpu,
    Mem,
    Disk,
    Gpu,
}

impl Metric {
    const ALL: [(Metric, &'static str, &'static str); 4] = [
        (Metric::Cpu, "cpu", "CPU"),
        (Metric::Mem, "mem", "Memory"),
        (Metric::Disk, "disk", "Disk"),
        (Metric::Gpu, "gpu", "GPU"),
    ];

    fn from_id(id: &str) -> Metric {
        Self::ALL.iter().find(|(_, i, _)| *i == id).map_or(Metric::Cpu, |(m, ..)| *m)
    }

    fn value(self, p: &Proc) -> f64 {
        match self {
            Metric::Cpu => p.cpu,
            Metric::Mem => p.rss as f64,
            Metric::Disk => p.read_bps + p.write_bps,
            Metric::Gpu => p.gpu,
        }
    }

    fn show(self, v: f64) -> String {
        match self {
            Metric::Cpu | Metric::Gpu => fmt::pct(v),
            Metric::Mem => fmt::bytes(v),
            Metric::Disk => fmt::rate(v),
        }
    }
}

struct TopRow {
    button: gtk::Button,
    name: gtk::Label,
    value: gtk::Label,
    pid: Rc<Cell<i32>>,
}

struct TopList {
    metric: Rc<Cell<Metric>>,
    rows: Vec<TopRow>,
    empty: gtk::Label,
}

/// A card listing the five busiest processes by a metric picked with chips.
/// `slot` is which of the two cards this is (its choice is saved).
fn top_list(slot: usize) -> (gtk::Box, TopList) {
    let card = widgets::vbox(8);
    card.add_css_class("graph-card");
    card.set_hexpand(true);
    let initial = Metric::from_id(prefs::get().overview_top.get(slot).map_or("cpu", String::as_str));
    let metric = Rc::new(Cell::new(initial));
    let head = widgets::hbox(6);
    head.append(&widgets::label("By", "graph-card-title"));
    let chips: Rc<RefCell<Vec<gtk::Button>>> = Rc::default();
    for (m, id, label) in Metric::ALL {
        let b = gtk::Button::with_label(label);
        b.add_css_class("chip");
        b.add_css_class("small");
        if m == initial {
            b.add_css_class("selected");
        }
        let (metric, chips2) = (metric.clone(), chips.clone());
        b.connect_clicked(move |me| {
            metric.set(m);
            for c in chips2.borrow().iter() {
                c.remove_css_class("selected");
            }
            me.add_css_class("selected");
            prefs::update(|p| {
                p.overview_top.resize(2, "cpu".into());
                p.overview_top[slot] = id.into();
            });
            live::poke();
        });
        head.append(&b);
        chips.borrow_mut().push(b);
    }
    card.append(&head);
    let rows_box = widgets::vbox(2);
    card.append(&rows_box);
    let empty = widgets::label("Nothing busy right now.", "dim");
    empty.set_visible(false);
    rows_box.append(&empty);
    let mut rows = Vec::new();
    for _ in 0..5 {
        let button = gtk::Button::new();
        button.add_css_class("flat");
        let r = widgets::hbox(10);
        let name = widgets::label("", "");
        name.set_hexpand(true);
        name.set_ellipsize(pango::EllipsizeMode::End);
        let value = widgets::label("", "cell-num");
        r.append(&name);
        r.append(&value);
        button.set_child(Some(&r));
        button.set_visible(false);
        let pid = Rc::new(Cell::new(0));
        let p = pid.clone();
        button.connect_clicked(move |_| super::processes::reveal(p.get()));
        rows_box.append(&button);
        rows.push(TopRow { button, name, value, pid });
    }
    (card, TopList { metric, rows, empty })
}

fn fill_top(list: &TopList, procs: &[&Proc]) {
    let m = list.metric.get();
    let mut sorted: Vec<&&Proc> = procs.iter().filter(|p| m.value(p) > 0.0).collect();
    sorted.sort_by(|a, b| m.value(b).total_cmp(&m.value(a)));
    list.empty.set_visible(sorted.is_empty());
    for (i, row) in list.rows.iter().enumerate() {
        match sorted.get(i) {
            Some(p) => {
                row.pid.set(p.pid);
                row.name.set_text(&p.name);
                row.value.set_text(&m.show(m.value(p)));
                row.button.set_tooltip_text(Some(&format!("Show {} (PID {}) in Processes", p.name, p.pid)));
                row.button.set_visible(true);
            }
            None => row.button.set_visible(false),
        }
    }
}

pub fn build(page: &Page) {
    let g = page.group("Usage");
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(4);
    flow.set_row_spacing(10);
    flow.set_column_spacing(10);
    g.add(&flow);

    let spark =
        |key: &str, scale: Scale, f: fn(f64) -> String| graph::sparkline(vec![Series::new(key, "", Tone::Accent)], scale, 38, f);
    let pair = |a: (&str, &str), b: (&str, &str), floor: f64, f: fn(f64) -> String| {
        graph::sparkline(
            vec![Series::new(a.0, a.1, Tone::Accent), Series::new(b.0, b.1, Tone::Second)],
            Scale::Auto { floor },
            38,
            f,
        )
    };
    let cpu = tile("CPU", "cpu", spark("cpu", Scale::Percent, fmt::pct));
    let mem = tile("Memory", "memory", spark("mem", Scale::Percent, fmt::pct));
    let gpu = tile("GPU", "gpu", spark("gpu", Scale::Percent, fmt::pct));
    let disk = tile("Disk", "storage", pair(("disk.read", "Read"), ("disk.write", "Write"), 1024.0 * 1024.0, fmt::rate));
    let net = tile("Network", "network", pair(("net.rx", "Down"), ("net.tx", "Up"), 64.0 * 1024.0, fmt::net_rate));
    let temp = tile("Temperature", "sensors", spark("temp.cpu", Scale::Auto { floor: 60.0 }, fmt::temp));
    let bat_name = live::latest().and_then(|s| s.sensors.batteries.first().map(|b| b.name.clone())).or_else(|| {
        std::fs::read_dir("/sys/class/power_supply")
            .ok()?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .find(|n| n.starts_with("BAT"))
    });
    let bat = bat_name.as_ref().map(|name| tile("Battery", "sensors", spark(&format!("bat.{name}"), Scale::Percent, fmt::pct)));

    let mut buttons: Vec<(&'static str, gtk::Button)> = vec![
        ("cpu", cpu.button.clone()),
        ("mem", mem.button.clone()),
        ("gpu", gpu.button.clone()),
        ("disk", disk.button.clone()),
        ("net", net.button.clone()),
        ("temp", temp.button.clone()),
    ];
    if let Some(b) = &bat {
        buttons.push(("bat", b.button.clone()));
    }
    let buttons = Rc::new(buttons);
    arrange(&flow, &buttons);
    g.header_end(&tile_editor(&flow, buttons));

    live::on_tick(&flow, move |s: &Snapshot| {
        let p = prefs::get();
        cpu.value.set_text(&fmt::pct(s.cpu.usage));
        let avg = if s.cpu.freqs.is_empty() { 0.0 } else { s.cpu.freqs.iter().sum::<f64>() / s.cpu.freqs.len() as f64 };
        cpu.sub.set_text(&format!("{} · {} threads", fmt::mhz(avg), s.cpu.cores.len()));

        let m = &s.mem;
        let mem_pct = if m.total > 0 { m.used as f64 / m.total as f64 * 100.0 } else { 0.0 };
        mem.value.set_text(&fmt::pct(mem_pct));
        widgets::set_warn(&mem.value, mem_pct >= p.warn_mem);
        mem.sub.set_text(&format!("{} of {}", fmt::bytes(m.used as f64), fmt::bytes(m.total as f64)));

        if let Some(g) = s.gpus.iter().max_by(|a, b| a.util.unwrap_or(-1.0).total_cmp(&b.util.unwrap_or(-1.0))) {
            gpu.value.set_text(&g.util.map(fmt::pct).unwrap_or_else(|| "0%".into()));
            let asleep = s.gpus.iter().filter(|g| g.power_state == crate::sampler::gpu::Power::Asleep).count();
            let mut sub = g.name.replace("NVIDIA ", "").replace("Intel ", "");
            if asleep > 0 {
                sub = format!("{sub} · {asleep} asleep");
            }
            gpu.sub.set_text(&sub);
        } else if s.procs.is_none() {
            gpu.sub.set_text("Paused while hidden");
        }

        disk.value.set_text(&fmt::pct(s.disk_busy()));
        disk.sub.set_text(&format!("↓ {}  ↑ {}", fmt::rate(s.disk_read()), fmt::rate(s.disk_write())));

        net.value.set_text(&fmt::net_rate(s.net_rx() + s.net_tx()));
        net.sub.set_text(&format!("↓ {}  ↑ {}", fmt::net_rate(s.net_rx()), fmt::net_rate(s.net_tx())));

        temp.value.set_text(&s.sensors.cpu_temp.map(fmt::temp).unwrap_or_else(|| "–".into()));
        widgets::set_warn(&temp.value, s.sensors.cpu_temp.is_some_and(|t| t >= p.warn_temp));
        let fan = s.sensors.fans.iter().map(|f| f.value).fold(0.0, f64::max);
        temp.sub.set_text(&if fan > 0.0 { format!("CPU · fans {} rpm", fan.round()) } else { "CPU package".into() });

        if let (Some(bat), Some(b)) = (&bat, s.sensors.batteries.first()) {
            bat.value.set_text(&fmt::pct(b.percent));
            let mut sub = b.status.clone();
            if let Some(h) = b.hours_left {
                sub = format!("{sub} · {} left", fmt::hours(h));
            }
            if b.watts > 0.1 {
                sub = format!("{sub} · {}", fmt::watts(b.watts));
            }
            bat.sub.set_text(&sub);
        }
    });

    // ----- System -----
    let g = page.group("System");
    let card = widgets::vbox(0);
    card.add_css_class("graph-card");
    let (kvs, v) = widgets::kv_flow(&["Uptime", "Load average", "Processes", "Threads", "Kernel", "Processor"]);
    card.append(&kvs);
    g.add(&card);
    let kernel = crate::sampler::read("/proc/sys/kernel/osrelease").trim().to_string();
    v[4].set_text(&kernel);
    v[4].set_tooltip_text(Some(&kernel));
    live::on_tick(&card, move |s| {
        v[0].set_text(&fmt::duration(s.cpu.uptime));
        v[1].set_text(&format!("{:.2} {:.2} {:.2}", s.cpu.load[0], s.cpu.load[1], s.cpu.load[2]));
        if s.process_count > 0 {
            v[2].set_text(&s.process_count.to_string());
            v[3].set_text(&s.thread_count.to_string());
        }
        v[5].set_text(&s.cpu.model);
        v[5].set_tooltip_text(Some(&s.cpu.model));
    });

    // ----- Busiest processes -----
    let g = page.group("Busiest processes");
    let pair = gtk::FlowBox::new();
    pair.set_selection_mode(gtk::SelectionMode::None);
    pair.set_homogeneous(true);
    pair.set_min_children_per_line(1);
    pair.set_max_children_per_line(2);
    pair.set_column_spacing(10);
    pair.set_row_spacing(10);
    let (c1, first) = top_list(0);
    let (c2, second) = top_list(1);
    pair.append(&c1);
    pair.append(&c2);
    let mut child = pair.first_child();
    while let Some(c) = child {
        c.set_focusable(false);
        child = c.next_sibling();
    }
    g.add(&pair);
    live::on_tick(&pair, move |s| {
        let Some(procs) = &s.procs else { return };
        let list: Vec<&Proc> = procs.iter().filter(|p| !p.kernel).collect();
        fill_top(&first, &list);
        fill_top(&second, &list);
    });
}
