use crate::graph::{self, Scale, Series, Tone};
use crate::sampler::net::{Iface, Kind};
use crate::widgets::{self, Page};
use crate::{fmt, live};
use gtk::prelude::*;

pub fn build(page: &Page) {
    let holder = widgets::vbox(0);
    page.body.append(&holder);
    live::ready(move |snap| {
        let shown: Vec<&Iface> = snap.nets.iter().filter(|n| n.kind != Kind::Virtual || n.up).collect();
        if shown.is_empty() {
            holder.append(&widgets::banner("No network interfaces are up.", false));
            return;
        }
        for n in shown {
            iface_section(&holder, n);
        }
    });
}

fn iface_section(body: &gtk::Box, n: &Iface) {
    let kind = match n.kind {
        Kind::Wifi => "Wi-Fi",
        Kind::Ethernet => "Ethernet",
        Kind::Virtual => "Virtual",
    };
    let title = widgets::label(&format!("{kind} · {}", n.name).to_uppercase(), "group-title");
    body.append(&title);
    let list = widgets::vbox(6);
    body.append(&list);
    let name = n.name.clone();
    let gr = graph::graph(
        vec![
            Series::new(format!("net.{name}.rx"), "Download", Tone::Accent),
            Series::new(format!("net.{name}.tx"), "Upload", Tone::Second),
        ],
        Scale::Auto { floor: 64.0 * 1024.0 },
        fmt::net_rate,
        140,
    );
    let (card, summary) = widgets::graph_card("Traffic", &gr.root);
    list.append(&card);
    let info = widgets::vbox(0);
    info.add_css_class("graph-card");
    let keys: Vec<&str> = if n.kind == Kind::Wifi {
        vec!["Network", "Signal", "Received", "Sent", "Addresses", "Hardware address"]
    } else {
        vec!["Link speed", "State", "Received", "Sent", "Addresses", "Hardware address"]
    };
    let (flow, v) = widgets::kv_flow(&keys);
    info.append(&flow);
    list.append(&info);
    let wifi = n.kind == Kind::Wifi;
    live::on_tick(&info, move |s| {
        let Some(n) = s.nets.iter().find(|x| x.name == name) else {
            summary.set_text("Gone");
            return;
        };
        summary.set_text(&format!("↓ {}  ↑ {}", fmt::net_rate(n.rx_bps), fmt::net_rate(n.tx_bps)));
        if wifi {
            v[0].set_text(n.ssid.as_deref().unwrap_or(if n.up { "–" } else { "Disconnected" }));
            v[1].set_text(&n.signal.map(|d| format!("{d:.0} dBm · {}", quality(d))).unwrap_or_else(|| "–".into()));
        } else {
            v[0].set_text(
                &n.speed_mbps
                    .map(|m| if m >= 1000 { format!("{} Gb/s", m / 1000) } else { format!("{m} Mb/s") })
                    .unwrap_or_else(|| "–".into()),
            );
            v[1].set_text(if n.up { "Connected" } else { "Down" });
        }
        v[2].set_text(&fmt::bytes(n.rx_total as f64));
        v[3].set_text(&fmt::bytes(n.tx_total as f64));
        let addrs = if n.addrs.is_empty() { "–".to_string() } else { n.addrs.join("\n") };
        v[4].set_text(n.addrs.first().map(String::as_str).unwrap_or("–"));
        v[4].set_tooltip_text(Some(&addrs));
        v[5].set_text(if n.mac.is_empty() { "–" } else { &n.mac });
    });
}

fn quality(dbm: f64) -> &'static str {
    match dbm {
        d if d >= -55.0 => "excellent",
        d if d >= -67.0 => "good",
        d if d >= -75.0 => "fair",
        _ => "weak",
    }
}
