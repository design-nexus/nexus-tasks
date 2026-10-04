use crate::graph::{self, Scale, Series, Tone};
use crate::widgets::{self, Page};
use crate::{fmt, live};
use gtk::prelude::*;

fn title(body: &gtk::Box, text: &str) {
    body.append(&widgets::label(&text.to_uppercase(), "group-title"));
}

pub fn build(page: &Page) {
    let holder = widgets::vbox(0);
    page.body.append(&holder);
    live::ready(move |snap| {
        let body = &holder;
        // ----- Battery -----
        for b in &snap.sensors.batteries {
            title(body, &format!("Battery · {}", b.name));
            let list = widgets::vbox(6);
            body.append(&list);
            let name = b.name.clone();
            let gr =
                graph::graph(vec![Series::new(format!("bat.{name}"), "Charge", Tone::Accent)], Scale::Percent, fmt::pct, 110);
            let (card, summary) = widgets::graph_card("Charge", &gr.root);
            list.append(&card);
            let gr = graph::graph(
                vec![Series::new(format!("bat.{name}.watts"), "Power", Tone::Accent)],
                Scale::Auto { floor: 10.0 },
                fmt::watts,
                90,
            );
            let (pcard, psummary) = widgets::graph_card("Power draw", &gr.root);
            list.append(&pcard);
            let info = widgets::vbox(0);
            info.add_css_class("graph-card");
            let (flow, v) =
                widgets::kv_flow(&["Charge", "State", "Power", "Time left", "Capacity", "Health", "Cycles", "Charger"]);
            info.append(&flow);
            list.append(&info);
            live::on_tick(&info, move |s| {
                let Some(b) = s.sensors.batteries.iter().find(|x| x.name == name) else { return };
                summary.set_text(&format!("{} · {}", fmt::pct(b.percent), b.status));
                psummary.set_text(&fmt::watts(b.watts));
                v[0].set_text(&fmt::pct(b.percent));
                v[1].set_text(&b.status);
                v[2].set_text(&fmt::watts(b.watts));
                v[3].set_text(&b.hours_left.map(fmt::hours).unwrap_or_else(|| "–".into()));
                v[4].set_text(&format!("{:.1} / {:.1} Wh", b.energy_now, b.energy_full));
                v[5].set_text(&b.health().map(fmt::pct).unwrap_or_else(|| "–".into()));
                v[6].set_text(&b.cycles.map(|c| c.to_string()).unwrap_or_else(|| "–".into()));
                v[7].set_text(match s.sensors.ac {
                    Some(true) => "Plugged in",
                    Some(false) => "Unplugged",
                    None => "–",
                });
            });
        }

        // ----- Temperatures -----
        title(body, "Temperatures");
        if snap.sensors.cpu_temp.is_some() {
            let gr =
                graph::graph(vec![Series::new("temp.cpu", "CPU", Tone::Accent)], Scale::Auto { floor: 60.0 }, fmt::temp, 120);
            let (card, summary) = widgets::graph_card("CPU package", &gr.root);
            body.append(&card);
            live::on_tick(&card, move |s| {
                summary.set_text(&s.sensors.cpu_temp.map(fmt::temp).unwrap_or_default());
                widgets::set_warn(&summary, s.sensors.cpu_temp.is_some_and(|t| t >= crate::prefs::get().warn_temp));
            });
        }
        if snap.sensors.temps.is_empty() {
            body.append(&widgets::row("No temperature sensors", "The kernel exposes no hwmon temperature inputs here.", None));
        } else {
            let flow = gtk::FlowBox::new();
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_homogeneous(true);
            flow.set_min_children_per_line(2);
            flow.set_max_children_per_line(5);
            flow.set_row_spacing(8);
            flow.set_column_spacing(8);
            flow.set_margin_top(8);
            let mut cells = Vec::new();
            for t in &snap.sensors.temps {
                let cell = widgets::vbox(4);
                cell.add_css_class("core-cell");
                let head = widgets::hbox(6);
                let n = widgets::label(&format!("{} · {}", t.chip, t.label), "core-name");
                n.set_hexpand(true);
                n.set_ellipsize(gtk::pango::EllipsizeMode::End);
                n.set_tooltip_text(Some(&format!("{} · {}", t.chip, t.label)));
                let v = widgets::label("", "core-value");
                v.add_css_class("mono");
                head.append(&n);
                head.append(&v);
                cell.append(&head);
                let bar = widgets::usage_bar();
                cell.append(&bar);
                flow.append(&cell);
                if let Some(c) = flow.last_child() {
                    c.set_focusable(false);
                }
                cells.push((t.chip.clone(), t.label.clone(), v, bar));
            }
            body.append(&flow);
            live::on_tick(&flow, move |s| {
                for (chip, label, v, bar) in &cells {
                    if let Some(t) = s.sensors.temps.iter().find(|t| &t.chip == chip && &t.label == label) {
                        v.set_text(&fmt::temp(t.value));
                        widgets::set_warn(v, t.value >= crate::prefs::get().warn_temp);
                        bar.set_value((t.value / t.high.unwrap_or(100.0)).clamp(0.0, 1.0));
                    }
                }
            });
        }

        // ----- Fans -----
        if !snap.sensors.fans.is_empty() {
            title(body, "Fans");
            let list = widgets::vbox(6);
            body.append(&list);
            let mut labels = Vec::new();
            for f in &snap.sensors.fans {
                let v = widgets::label("", "value-readout");
                v.set_xalign(1.0);
                list.append(&widgets::row(&format!("{} · {}", f.chip, f.label), "", Some(v.upcast_ref())));
                labels.push((f.chip.clone(), f.label.clone(), v));
            }
            live::on_tick(&list, move |s| {
                for (chip, label, v) in &labels {
                    if let Some(f) = s.sensors.fans.iter().find(|f| &f.chip == chip && &f.label == label) {
                        v.set_text(&format!("{} rpm", f.value.round()));
                    }
                }
            });
        }
    });
}
