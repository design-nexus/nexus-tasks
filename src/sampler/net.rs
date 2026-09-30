//! Network interfaces: rates from `/proc/net/dev`, addresses and Wi-Fi details.

use super::read;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Counters {
    pub rx: u64,
    pub tx: u64,
}

pub fn parse_net_dev(text: &str) -> HashMap<String, Counters> {
    text.lines()
        .skip(2)
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let f: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            if f.len() < 9 {
                return None;
            }
            Some((name.trim().to_string(), Counters { rx: f[0], tx: f[8] }))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    Wifi,
    #[default]
    Ethernet,
    Virtual,
}

#[derive(Debug, Clone, Default)]
pub struct Iface {
    pub name: String,
    pub kind: Kind,
    pub up: bool,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub rx_total: u64,
    pub tx_total: u64,
    pub addrs: Vec<String>,
    pub mac: String,
    pub ssid: Option<String>,
    /// Signal level in dBm.
    pub signal: Option<f64>,
    pub speed_mbps: Option<u64>,
}

fn kind(name: &str) -> Kind {
    let base = format!("/sys/class/net/{name}");
    if Path::new(&format!("{base}/wireless")).exists() {
        Kind::Wifi
    } else if Path::new(&format!("{base}/device")).exists() {
        Kind::Ethernet
    } else {
        Kind::Virtual
    }
}

/// Addresses by interface, from `ip -j addr`.
pub fn parse_ip_json(text: &str) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(text) else { return out };
    for iface in list {
        let Some(name) = iface["ifname"].as_str() else { continue };
        let addrs: Vec<String> = iface["addr_info"]
            .as_array()
            .map(|a| a.iter().filter_map(|i| Some(format!("{}/{}", i["local"].as_str()?, i["prefixlen"].as_u64()?))).collect())
            .unwrap_or_default();
        out.insert(name.to_string(), addrs);
    }
    out
}

/// Signal level by interface from `/proc/net/wireless`.
pub fn parse_wireless(text: &str) -> HashMap<String, f64> {
    text.lines()
        .skip(2)
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let f: Vec<&str> = rest.split_whitespace().collect();
            let level: f64 = f.get(2)?.trim_end_matches('.').parse().ok()?;
            Some((name.trim().to_string(), level))
        })
        .collect()
}

/// SSID and signal (dBm) from `iw dev X link`.
pub fn parse_iw_link(text: &str) -> (Option<String>, Option<f64>) {
    let mut ssid = None;
    let mut signal = None;
    for l in text.lines().map(str::trim) {
        if let Some(v) = l.strip_prefix("SSID: ") {
            ssid = Some(v.to_string());
        } else if let Some(v) = l.strip_prefix("signal: ") {
            signal = v.split_whitespace().next().and_then(|n| n.parse().ok());
        }
    }
    (ssid, signal)
}

fn output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

pub struct NetSampler {
    prev: HashMap<String, Counters>,
    tick: u64,
    addrs: HashMap<String, Vec<String>>,
    ssids: HashMap<String, (String, Option<f64>)>,
}

impl NetSampler {
    pub fn new() -> Self {
        NetSampler { prev: HashMap::new(), tick: 0, addrs: HashMap::new(), ssids: HashMap::new() }
    }

    pub fn sample(&mut self, elapsed: f64) -> Vec<Iface> {
        let stats = parse_net_dev(&read("/proc/net/dev"));
        let slow = self.tick.is_multiple_of(10);
        self.tick += 1;
        if slow {
            self.addrs = parse_ip_json(&output("ip", &["-j", "addr"]));
            self.ssids.clear();
        }
        let signals = parse_wireless(&read("/proc/net/wireless"));
        let mut out: Vec<Iface> = Vec::new();
        for (name, now) in &stats {
            if name == "lo" || name.starts_with("veth") {
                continue;
            }
            let base = format!("/sys/class/net/{name}");
            let k = kind(name);
            let mut i = Iface {
                name: name.clone(),
                kind: k,
                up: read(&format!("{base}/operstate")).trim() == "up",
                rx_total: now.rx,
                tx_total: now.tx,
                addrs: self.addrs.get(name).cloned().unwrap_or_default(),
                mac: read(&format!("{base}/address")).trim().to_string(),
                signal: signals.get(name).copied(),
                speed_mbps: read(&format!("{base}/speed")).trim().parse::<i64>().ok().filter(|s| *s > 0).map(|s| s as u64),
                ..Default::default()
            };
            if let Some(prev) = self.prev.get(name)
                && elapsed > 0.0
            {
                i.rx_bps = now.rx.saturating_sub(prev.rx) as f64 / elapsed;
                i.tx_bps = now.tx.saturating_sub(prev.tx) as f64 / elapsed;
            }
            if k == Kind::Wifi && i.up {
                if slow && !self.ssids.contains_key(name) {
                    let (ssid, signal) = parse_iw_link(&output("iw", &["dev", name, "link"]));
                    if let Some(ssid) = ssid {
                        self.ssids.insert(name.clone(), (ssid, signal));
                    }
                }
                if let Some((ssid, signal)) = self.ssids.get(name) {
                    i.ssid = Some(ssid.clone());
                    // Newer drivers leave /proc/net/wireless empty; iw still knows.
                    i.signal = i.signal.or(*signal);
                }
            }
            out.push(i);
        }
        // Physical first, then by name.
        out.sort_by(|a, b| (a.kind == Kind::Virtual, !a.up, &a.name).cmp(&(b.kind == Kind::Virtual, !b.up, &b.name)));
        self.prev = stats;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_net_dev() {
        let text = "Inter-|   Receive\n face |bytes packets\n\
                    wlo1: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\n    lo: 5 1 0 0 0 0 0 0 5 1 0 0 0 0 0 0\n";
        let s = parse_net_dev(text);
        assert_eq!(s["wlo1"], Counters { rx: 1000, tx: 2000 });
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn parses_ip_json() {
        let text = r#"[{"ifname":"wlo1","addr_info":[{"family":"inet","local":"192.168.1.5","prefixlen":24}]}]"#;
        assert_eq!(parse_ip_json(text)["wlo1"], vec!["192.168.1.5/24".to_string()]);
    }

    #[test]
    fn parses_iw_link() {
        let text = "Connected to aa:bb (on wlo1)\n\tSSID: SkyNet\n\tfreq: 5180\n\tsignal: -52 dBm\n";
        assert_eq!(parse_iw_link(text), (Some("SkyNet".into()), Some(-52.0)));
    }

    #[test]
    fn parses_wireless() {
        let text = "Inter-| sta-|\n face | tus |\n  wlo1: 0000   60.  -50.  -256        0      0\n";
        assert_eq!(parse_wireless(text)["wlo1"], -50.0);
    }
}
