use crate::graph::{self, Scale, Series, Tone};
use crate::sampler::disk::Disk;
use crate::widgets::{self, Page};
use crate::{cmd, fmt, live};
use gtk::pango;
use gtk::prelude::*;

fn kind(d: &Disk) -> &'static str {
    if d.name.starts_with("nvme") {
        "NVMe SSD"
    } else if d.removable {
        "Removable"
    } else if d.rotational {
        "Hard disk"
    } else {
        "SSD"
    }
}

fn mount_row(path: &str) -> (gtk::Box, gtk::LevelBar, gtk::Label) {
    let row = widgets::vbox(6);
    row.add_css_class("settings-option");
    let head = widgets::hbox(10);
    let p = widgets::label(path, "settings-option-title");
    p.add_css_class("mono");
    p.set_hexpand(true);
    p.set_ellipsize(pango::EllipsizeMode::Middle);
    let open = gtk::Button::from_icon_name("folder-open-symbolic");
    open.add_css_class("flat");
    open.set_tooltip_text(Some("Open in the file manager"));
    let target = path.to_string();
    open.connect_clicked(move |_| cmd::spawn(&["xdg-open", &target]));
    let detail = widgets::label("", "dim");
    detail.add_css_class("mono");
    head.append(&p);
    head.append(&detail);
    head.append(&open);
    row.append(&head);
    let bar = widgets::usage_bar();
    row.append(&bar);
    (row, bar, detail)
}

pub fn build(page: &Page) {
    let body = page.body.clone();
    let holder = widgets::vbox(0);
    body.append(&holder);
    let page_body = holder.clone();
    live::ready(move |snap| {
        if snap.disks.is_empty() {
            page_body.append(&widgets::banner("No disks were found in <tt>/sys/block</tt>.", true));
            return;
        }
        // Everything together first, when there's more than one disk.
        if snap.disks.len() > 1 {
            page_body.append(&group_title("All disks"));
            let gr = graph::graph(
                vec![Series::new("disk.read", "Read", Tone::Accent), Series::new("disk.write", "Write", Tone::Second)],
                Scale::Auto { floor: 1024.0 * 1024.0 },
                fmt::rate,
                130,
            );
            let (card, summary) = widgets::graph_card("Throughput", &gr.root);
            page_body.append(&card);
            live::on_tick(&card, move |s| summary.set_text(&format!("busiest {}", fmt::pct(s.disk_busy()))));
        }
        for d in &snap.disks {
            disk_section(&page_body, d);
        }
    });
}

fn group_title(text: &str) -> gtk::Label {
    let l = widgets::label(&text.to_uppercase(), "group-title");
    l.set_wrap(true);
    l
}

fn disk_section(body: &gtk::Box, d: &Disk) {
    body.append(&group_title(&format!("{} · {}", d.model, d.name)));
    let list = widgets::vbox(6);
    body.append(&list);
    let name = d.name.clone();
    let gr = graph::graph(
        vec![
            Series::new(format!("disk.{name}.read"), "Read", Tone::Accent),
            Series::new(format!("disk.{name}.write"), "Write", Tone::Second),
        ],
        Scale::Auto { floor: 1024.0 * 1024.0 },
        fmt::rate,
        130,
    );
    let (card, summary) = widgets::graph_card(&format!("{} · {}", kind(d), fmt::bytes(d.size as f64)), &gr.root);
    list.append(&card);
    let info = widgets::vbox(0);
    info.add_css_class("graph-card");
    let (flow, v) = widgets::kv_flow(&["Active time", "Read", "Write", "Temperature", "Read since boot", "Written since boot"]);
    info.append(&flow);
    list.append(&info);

    let mut mounts = Vec::new();
    for m in &d.mounts {
        let (row, bar, detail) = mount_row(&m.path);
        row.set_tooltip_text(Some(&format!("{} · {}", m.device, m.fs)));
        list.append(&row);
        mounts.push((m.path.clone(), bar, detail));
    }

    live::on_tick(&info, move |s| {
        let Some(d) = s.disks.iter().find(|x| x.name == name) else { return };
        summary.set_text(&format!("{} active", fmt::pct(d.busy)));
        v[0].set_text(&fmt::pct(d.busy));
        v[1].set_text(&fmt::rate(d.read_bps));
        v[2].set_text(&fmt::rate(d.write_bps));
        v[3].set_text(&d.temp.map(fmt::temp).unwrap_or_else(|| "–".into()));
        v[4].set_text(&fmt::bytes(d.read_total as f64));
        v[5].set_text(&fmt::bytes(d.written_total as f64));
        for (path, bar, detail) in &mounts {
            if let Some(m) = d.mounts.iter().find(|m| &m.path == path)
                && m.total > 0
            {
                bar.set_value(m.used() as f64 / m.total as f64);
                detail.set_text(&format!("{} free of {}", fmt::bytes(m.free as f64), fmt::bytes(m.total as f64)));
            }
        }
    });
}
