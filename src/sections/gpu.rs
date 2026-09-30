use crate::graph::{self, Scale, Series, Tone};
use crate::sampler::gpu::{Gpu, Power, Vendor};
use crate::widgets::{self, Page};
use crate::{fmt, live};
use gtk::pango;
use gtk::prelude::*;

pub fn build(page: &Page) {
    let holder = widgets::vbox(0);
    page.body.append(&holder);
    live::ready(move |snap| {
        if snap.gpus.is_empty() {
            holder.append(&widgets::banner(
                "No GPU was found under <tt>/sys/class/drm</tt>, or sampling is paused while the window is hidden.",
                false,
            ));
            return;
        }
        for g in &snap.gpus {
            gpu_section(&holder, g);
        }
    });
}

fn gpu_section(body: &gtk::Box, g: &Gpu) {
    let title = widgets::label(&g.name.to_uppercase(), "group-title");
    title.set_wrap(true);
    body.append(&title);
    let list = widgets::vbox(6);
    body.append(&list);
    let pci = g.pci.clone();

    // Honest state card for a GPU we deliberately don't poll.
    let asleep_desc = if g.vendor == Vendor::Nvidia {
        "It's powered down to save battery. Asking it for statistics would wake it, so Tasks waits until an app starts using it."
    } else {
        "It's powered down to save energy."
    };
    let idle_desc = "Awake but unused. Tasks doesn't poll it now, so it can go back to sleep.";
    let sleep_row = widgets::row("Asleep", asleep_desc, None);
    list.append(&sleep_row);

    let gr = graph::graph(vec![Series::new(format!("gpu.{pci}.util"), "Busy", Tone::Accent)], Scale::Percent, fmt::pct, 150);
    let (card, summary) = widgets::graph_card("Utilization", &gr.root);
    list.append(&card);

    let vram_card = (g.vram_total.is_some() || g.vendor == Vendor::Nvidia).then(|| {
        let gr = graph::graph(vec![Series::new(format!("gpu.{pci}.vram"), "Used", Tone::Accent)], Scale::Percent, fmt::pct, 90);
        let (card, summary) = widgets::graph_card("Video memory", &gr.root);
        list.append(&card);
        (card, summary)
    });

    let info = widgets::vbox(0);
    info.add_css_class("graph-card");
    let (flow, v) = widgets::kv_flow(&[
        "Utilization",
        "Clock",
        "Video memory",
        "Temperature",
        "Power",
        "Apps using it",
        "Driver",
        "PCI address",
    ]);
    info.append(&flow);
    list.append(&info);

    let procs_card = widgets::vbox(4);
    procs_card.add_css_class("graph-card");
    procs_card.append(&widgets::label("Processes using it", "graph-card-title"));
    let procs_list = widgets::vbox(2);
    procs_card.append(&procs_list);
    list.append(&procs_card);

    let vendor = g.vendor;
    live::on_tick(&info, move |s| {
        let Some(g) = s.gpus.iter().find(|x| x.pci == pci) else { return };
        let asleep = g.power_state == Power::Asleep;
        let idle = g.power_state == Power::Idle;
        sleep_row.set_visible(asleep || idle);
        // The row's text column holds the title, then the description.
        let text = sleep_row.first_child();
        if let Some(t) = text.as_ref().and_then(|c| c.first_child()).and_downcast::<gtk::Label>() {
            t.set_text(if asleep { "Asleep" } else { "Idle" });
        }
        if let Some(d) = text.as_ref().and_then(|c| c.last_child()).and_downcast::<gtk::Label>() {
            d.set_text(if asleep { asleep_desc } else { idle_desc });
        }
        card.set_visible(!asleep);
        if let Some((c, sum)) = &vram_card {
            c.set_visible(!asleep && g.vram_total.is_some());
            if let (Some(u), Some(t)) = (g.vram_used, g.vram_total) {
                sum.set_text(&format!("{} of {}", fmt::bytes(u as f64), fmt::bytes(t as f64)));
            }
        }
        summary.set_text(&match g.power_state {
            Power::Asleep => "asleep".into(),
            Power::Idle => "idle".into(),
            Power::Active => g.util.map(fmt::pct).unwrap_or_default(),
        });
        let dash = || "–".to_string();
        v[0].set_text(&g.util.map(fmt::pct).unwrap_or_else(dash));
        v[1].set_text(&match (g.clock, g.max_clock) {
            (Some(c), Some(m)) if m > 0.0 => format!("{} / {}", fmt::mhz(c), fmt::mhz(m)),
            (Some(c), _) => fmt::mhz(c),
            _ => dash(),
        });
        v[2].set_text(&match (g.vram_used, g.vram_total) {
            (Some(u), Some(t)) => format!("{} / {}", fmt::bytes(u as f64), fmt::bytes(t as f64)),
            _ if vendor == Vendor::Intel => "Shared with RAM".into(),
            _ => dash(),
        });
        v[3].set_text(&g.temp.map(fmt::temp).unwrap_or_else(dash));
        v[4].set_text(&match (g.watts, g.watts_limit) {
            (Some(w), Some(l)) => format!("{} / {}", fmt::watts(w), fmt::watts(l)),
            (Some(w), None) => fmt::watts(w),
            _ => dash(),
        });
        v[5].set_text(&g.clients.to_string());
        v[6].set_text(&g.driver);
        v[7].set_text(&g.pci);

        // Who's using it.
        while let Some(c) = procs_list.first_child() {
            procs_list.remove(&c);
        }
        let Some(procs) = &s.procs else { return };
        let mut users: Vec<&crate::sampler::procs::Proc> =
            procs.iter().filter(|p| if vendor == Vendor::Nvidia { p.nvidia } else { p.gpu > 0.05 }).collect();
        users.sort_by(|a, b| b.gpu.total_cmp(&a.gpu));
        procs_card.set_visible(!users.is_empty());
        for p in users.into_iter().take(8) {
            let r = widgets::hbox(10);
            let n = widgets::label(&p.name, "");
            n.set_hexpand(true);
            n.set_ellipsize(pango::EllipsizeMode::End);
            r.append(&n);
            r.append(&widgets::label(&format!("PID {}", p.pid), "cell-dim"));
            let value = if vendor == Vendor::Nvidia { "open".to_string() } else { fmt::pct(p.gpu) };
            let val = widgets::label(&value, "cell-num");
            val.set_width_chars(6);
            val.set_xalign(1.0);
            r.append(&val);
            procs_list.append(&r);
        }
    });
}
