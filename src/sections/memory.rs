use crate::graph::{self, Scale, Series, Tone};
use crate::widgets::{self, Page};
use crate::{fmt, live};
use gtk::prelude::*;

pub fn build(page: &Page) {
    let g = page.group("Memory");
    let gr = graph::graph(
        vec![Series::new("mem", "In use", Tone::Accent), Series::new("mem.cached", "With cache", Tone::Second)],
        Scale::Percent,
        fmt::pct,
        190,
    );
    let (card, summary) = widgets::graph_card("RAM", &gr.root);
    g.add(&card);

    // Composition: in use / cache / free, as one bar.
    let comp = widgets::vbox(8);
    comp.add_css_class("graph-card");
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("composition");
    bar.set_overflow(gtk::Overflow::Hidden);
    let used = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    used.add_css_class("part-used");
    let cached = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    cached.add_css_class("part-cached");
    bar.append(&used);
    bar.append(&cached);
    let head = widgets::hbox(16);
    let title = widgets::label("Composition", "graph-card-title");
    title.set_hexpand(true);
    head.append(&title);
    for (class, text) in [("part-used", "In use"), ("part-cached", "Cached"), ("", "Free")] {
        let item = widgets::hbox(6);
        let sw = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sw.add_css_class("legend-key");
        if !class.is_empty() {
            sw.add_css_class(class);
        }
        sw.set_valign(gtk::Align::Center);
        item.append(&sw);
        item.append(&widgets::label(text, "legend-label"));
        head.append(&item);
    }
    comp.append(&head);
    comp.append(&bar);
    let (flow, v) = widgets::kv_flow(&["In use", "Cached", "Available", "Buffers", "Shared", "Dirty", "Total"]);
    comp.append(&flow);
    g.add(&comp);

    let b2 = bar.clone();
    live::on_tick(&comp, move |s| {
        let m = &s.mem;
        summary.set_text(&format!("{} of {}", fmt::bytes(m.used as f64), fmt::bytes(m.total as f64)));
        let w = b2.width().max(1) as f64;
        if m.total > 0 {
            used.set_size_request((m.used as f64 / m.total as f64 * w) as i32, 10);
            cached.set_size_request((m.cached.min(m.total - m.used) as f64 / m.total as f64 * w) as i32, 10);
        }
        for (label, value) in v.iter().zip([m.used, m.cached, m.available, m.buffers, m.shared, m.dirty, m.total]) {
            label.set_text(&fmt::bytes(value as f64));
        }
    });

    // ----- Swap -----
    let g = page.group("Swap");
    let has_swap = live::latest().map(|s| s.mem.swap_total > 0).unwrap_or(true);
    if !has_swap {
        g.add(&widgets::row(
            "No swap",
            "This system has no swap or zram. When RAM runs out, the kernel ends processes instead.",
            None,
        ));
        return;
    }
    g.note("Memory moved out of RAM. On Omarchy this is usually zram: compressed memory, not disk.");
    let gr = graph::graph(vec![Series::new("swap", "Swap", Tone::Accent)], Scale::Percent, fmt::pct, 110);
    let (card, summary) = widgets::graph_card("Swap in use", &gr.root);
    g.add(&card);
    let info = widgets::vbox(0);
    info.add_css_class("graph-card");
    let (flow, v) = widgets::kv_flow(&["Used", "Total", "Compressed data", "RAM it takes", "Ratio"]);
    info.append(&flow);
    g.add(&info);
    live::on_tick(&info, move |s| {
        let m = &s.mem;
        summary.set_text(&format!("{} of {}", fmt::bytes(m.swap_used as f64), fmt::bytes(m.swap_total as f64)));
        v[0].set_text(&fmt::bytes(m.swap_used as f64));
        v[1].set_text(&fmt::bytes(m.swap_total as f64));
        v[2].set_text(&if m.zram_data > 0 { fmt::bytes(m.zram_data as f64) } else { "–".into() });
        v[3].set_text(&if m.zram_used > 0 { fmt::bytes(m.zram_used as f64) } else { "–".into() });
        v[4].set_text(&if m.zram_used > 0 { format!("{:.1}×", m.zram_data as f64 / m.zram_used as f64) } else { "–".into() });
    });
}
