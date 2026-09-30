use crate::graph::{self, Scale, Series, Tone};
use crate::sampler::Snapshot;
use crate::widgets::{self, Page};
use crate::{fmt, live, window};
use gtk::pango;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

struct Tile {
    value: gtk::Label,
    sub: gtk::Label,
}

fn tile(flow: &gtk::FlowBox, title: &str, target: &'static str, spark: gtk::DrawingArea) -> Tile {
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
    flow.append(&b);
    if let Some(child) = flow.last_child() {
        child.set_focusable(false);
    }
    Tile { value, sub }
}

struct TopRow {
    button: gtk::Button,
    name: gtk::Label,
    value: gtk::Label,
    pid: Rc<Cell<i32>>,
}

fn top_list(title: &str) -> (gtk::Box, Vec<TopRow>) {
    let card = widgets::vbox(8);
    card.add_css_class("graph-card");
    card.append(&widgets::label(title, "graph-card-title"));
    let rows = widgets::vbox(2);
    card.append(&rows);
    card.set_hexpand(true);
    let mut out = Vec::new();
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
        rows.append(&button);
        out.push(TopRow { button, name, value, pid });
    }
    (card, out)
}

fn fill_top(rows: &[TopRow], items: Vec<(i32, String, String)>) {
    for (i, row) in rows.iter().enumerate() {
        match items.get(i) {
            Some((pid, name, value)) => {
                row.pid.set(*pid);
                row.name.set_text(name);
                row.value.set_text(value);
                row.button.set_tooltip_text(Some(&format!("Show {name} (PID {pid}) in Processes")));
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

    let cpu = tile(&flow, "CPU", "cpu", graph::sparkline("cpu", Tone::Accent, Scale::Percent, 38));
    let mem = tile(&flow, "Memory", "memory", graph::sparkline("mem", Tone::Accent, Scale::Percent, 38));
    let gpu = tile(&flow, "GPU", "gpu", graph::sparkline("gpu", Tone::Accent, Scale::Percent, 38));
    let disk = tile(
        &flow,
        "Disk",
        "storage",
        graph::sparkline_multi(
            vec![Series::new("disk.read", "Read", Tone::Accent), Series::new("disk.write", "Write", Tone::Second)],
            Scale::Auto { floor: 1024.0 * 1024.0 },
            38,
        ),
    );
    let net = tile(
        &flow,
        "Network",
        "network",
        graph::sparkline_multi(
            vec![Series::new("net.rx", "Down", Tone::Accent), Series::new("net.tx", "Up", Tone::Second)],
            Scale::Auto { floor: 64.0 * 1024.0 },
            38,
        ),
    );
    let temp = tile(&flow, "Temperature", "sensors", graph::sparkline("temp.cpu", Tone::Accent, Scale::Auto { floor: 60.0 }, 38));
    let bat_name = live::latest().and_then(|s| s.sensors.batteries.first().map(|b| b.name.clone())).or_else(|| {
        std::fs::read_dir("/sys/class/power_supply")
            .ok()?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .find(|n| n.starts_with("BAT"))
    });
    let bat = bat_name.as_ref().map(|name| {
        tile(&flow, "Battery", "sensors", graph::sparkline(&format!("bat.{name}"), Tone::Accent, Scale::Percent, 38))
    });

    live::on_tick(&flow, move |s: &Snapshot| {
        cpu.value.set_text(&fmt::pct(s.cpu.usage));
        let avg = if s.cpu.freqs.is_empty() { 0.0 } else { s.cpu.freqs.iter().sum::<f64>() / s.cpu.freqs.len() as f64 };
        cpu.sub.set_text(&format!("{} · {} threads", fmt::mhz(avg), s.cpu.cores.len()));

        let m = &s.mem;
        mem.value.set_text(&fmt::pct(if m.total > 0 { m.used as f64 / m.total as f64 * 100.0 } else { 0.0 }));
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
    let (c1, by_cpu) = top_list("By CPU");
    let (c2, by_mem) = top_list("By memory");
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
        let mut list: Vec<&crate::sampler::procs::Proc> = procs.iter().filter(|p| !p.kernel).collect();
        list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        fill_top(&by_cpu, list.iter().take(5).map(|p| (p.pid, p.name.clone(), fmt::pct(p.cpu))).collect());
        list.sort_by_key(|a| std::cmp::Reverse(a.rss));
        fill_top(&by_mem, list.iter().take(5).map(|p| (p.pid, p.name.clone(), fmt::bytes(p.rss as f64))).collect());
    });
}
