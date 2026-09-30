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
    g.note("Busy time of every logical processor, with its current clock.");
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(8);
    flow.set_row_spacing(8);
    flow.set_column_spacing(8);
    g.add(&flow);
    let threads = live::latest()
        .map(|s| s.cpu.cores.len())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
    let mut values = Vec::new();
    for i in 0..threads {
        let cell = widgets::vbox(2);
        cell.add_css_class("core-cell");
        let head = widgets::hbox(6);
        let name = widgets::label(&format!("CPU {i}"), "core-name");
        name.set_hexpand(true);
        let value = widgets::label("", "core-value");
        value.add_css_class("mono");
        head.append(&name);
        head.append(&value);
        cell.append(&head);
        cell.append(&graph::sparkline(&format!("cpu.{i}"), Tone::Accent, Scale::Percent, 34));
        flow.append(&cell);
        if let Some(c) = flow.last_child() {
            c.set_focusable(false);
        }
        values.push(value);
    }
    live::on_tick(&flow, move |s| {
        for (i, v) in values.iter().enumerate() {
            let busy = s.cpu.cores.get(i).copied().unwrap_or(0.0);
            let freq = s.cpu.freqs.get(i).copied().unwrap_or(0.0);
            v.set_text(&format!("{} · {:.1}", fmt::pct(busy), freq / 1000.0));
            v.set_tooltip_text(Some(&format!("{} busy at {}", fmt::pct(busy), fmt::mhz(freq))));
        }
    });
}
