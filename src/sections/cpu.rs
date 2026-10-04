use crate::graph::{self, Scale, Series, Tone};
use crate::widgets::{self, Page};
use crate::{fmt, live};
use gtk::prelude::*;

pub fn build(page: &Page) {
    let g = page.group("Usage");
    let gr = graph::graph(
        vec![Series::new("cpu", "Total", Tone::Accent), Series::new("cpu.system", "Kernel", Tone::Second)],
        Scale::Percent,
        fmt::pct,
        190,
    );
    let (card, summary) = widgets::graph_card("All cores", &gr.root);
    g.add(&card);

    let info = widgets::vbox(0);
    info.add_css_class("graph-card");
    let (flow, v) = widgets::kv_flow(&[
        "Utilization",
        "Speed",
        "Max speed",
        "Cores",
        "Load average",
        "Temperature",
        "Running now",
        "Switches / s",
        "Governor",
        "Uptime",
    ]);
    info.append(&flow);
    g.add(&info);

    live::on_tick(&info, move |s| {
        let c = &s.cpu;
        summary.set_text(&c.model);
        v[0].set_text(&fmt::pct(c.usage));
        let avg = if c.freqs.is_empty() { 0.0 } else { c.freqs.iter().sum::<f64>() / c.freqs.len() as f64 };
        v[1].set_text(&fmt::mhz(avg));
        v[2].set_text(&if c.max_freq > 0.0 { fmt::mhz(c.max_freq) } else { "–".into() });
        v[3].set_text(&if c.physical > 0 {
            format!("{} / {} threads", c.physical, c.cores.len())
        } else {
            format!("{} threads", c.cores.len())
        });
        v[4].set_text(&format!("{:.2} {:.2} {:.2}", c.load[0], c.load[1], c.load[2]));
        v[5].set_text(&s.sensors.cpu_temp.map(fmt::temp).unwrap_or_else(|| "–".into()));
        v[6].set_text(&c.running.to_string());
        v[7].set_text(&format!("{:.0}", c.ctxt_per_sec));
        v[8].set_text(if c.governor.is_empty() { "–" } else { &c.governor });
        v[9].set_text(&fmt::duration(c.uptime));
    });

    // ----- Per core -----
    let g = page.group("Each thread");
    let grid = crate::prefs::get().cpu_grid;
    g.note(if grid {
        "Busy time of every logical processor; darker is busier."
    } else {
        "Busy time of every logical processor, with its current clock."
    });
    let switch = widgets::hbox(4);
    for (label, as_grid) in [("Graphs", false), ("Grid", true)] {
        let b = gtk::Button::with_label(label);
        b.add_css_class("chip");
        b.add_css_class("small");
        if as_grid == grid {
            b.add_css_class("selected");
        }
        b.connect_clicked(move |_| {
            if crate::prefs::get().cpu_grid != as_grid {
                crate::prefs::update(|p| p.cpu_grid = as_grid);
                crate::window::rebuild("cpu");
            }
        });
        switch.append(&b);
    }
    g.header_end(&switch);
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_min_children_per_line(if grid { 4 } else { 2 });
    flow.set_max_children_per_line(if grid { 16 } else { 8 });
    flow.set_row_spacing(if grid { 6 } else { 8 });
    flow.set_column_spacing(if grid { 6 } else { 8 });
    g.add(&flow);
    let threads = live::latest()
        .map(|s| s.cpu.cores.len())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
    let mut values = Vec::new();
    for i in 0..threads {
        let cell = widgets::vbox(2);
        cell.add_css_class(if grid { "core-tile" } else { "core-cell" });
        let name = widgets::label(&format!("CPU {i}"), "core-name");
        let value = widgets::label("", "core-value");
        value.add_css_class("mono");
        if grid {
            cell.append(&name);
            cell.append(&value);
        } else {
            let head = widgets::hbox(6);
            name.set_hexpand(true);
            head.append(&name);
            head.append(&value);
            cell.append(&head);
            cell.append(&graph::sparkline(
                vec![graph::Series::new(format!("cpu.{i}"), "", Tone::Accent)],
                Scale::Percent,
                34,
                fmt::pct,
            ));
        }
        flow.append(&cell);
        if let Some(c) = flow.last_child() {
            c.set_focusable(false);
        }
        values.push((cell, value));
    }
    live::on_tick(&flow, move |s| {
        for (i, (cell, v)) in values.iter().enumerate() {
            let busy = s.cpu.cores.get(i).copied().unwrap_or(0.0);
            let freq = s.cpu.freqs.get(i).copied().unwrap_or(0.0);
            if grid {
                v.set_text(&fmt::pct(busy));
                let level = [10.0, 30.0, 50.0, 70.0, 90.0].iter().filter(|t| busy >= **t).count();
                for (n, class) in ["heat-1", "heat-2", "heat-3", "heat-4", "heat-5"].iter().enumerate() {
                    if n + 1 == level {
                        cell.add_css_class(class);
                    } else {
                        cell.remove_css_class(class);
                    }
                }
            } else {
                v.set_text(&format!("{} · {:.1}", fmt::pct(busy), freq / 1000.0));
            }
            cell.set_tooltip_text(Some(&format!("CPU {i}: {} busy at {}", fmt::pct(busy), fmt::mhz(freq))));
        }
    });
}
