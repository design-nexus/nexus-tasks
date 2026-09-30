//! CPU usage from `/proc/stat`, frequencies from cpufreq, load and uptime.

use super::read;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Times {
    pub user: u64,
    pub system: u64,
    pub idle: u64,
    pub total: u64,
}

impl Times {
    fn parse(fields: &[&str]) -> Option<Times> {
        let n: Vec<u64> = fields.iter().take(8).map(|f| f.parse().ok()).collect::<Option<_>>()?;
        if n.len() < 5 {
            return None;
        }
        let get = |i: usize| n.get(i).copied().unwrap_or(0);
        // user nice system idle iowait irq softirq steal
        Some(Times { user: get(0) + get(1), system: get(2) + get(5) + get(6), idle: get(3) + get(4), total: n.iter().sum() })
    }
}

/// Parsed `/proc/stat`: the aggregate line, each core, and context switches.
#[derive(Debug, Clone, Default)]
pub struct Stat {
    pub total: Times,
    pub cores: Vec<Times>,
    pub ctxt: u64,
    pub running: u64,
}

pub fn parse_stat(text: &str) -> Stat {
    let mut stat = Stat::default();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let rest: Vec<&str> = parts.collect();
        if key == "cpu" {
            stat.total = Times::parse(&rest).unwrap_or_default();
        } else if key.starts_with("cpu") {
            if let Some(t) = Times::parse(&rest) {
                stat.cores.push(t);
            }
        } else if key == "ctxt" {
            stat.ctxt = rest.first().and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if key == "procs_running" {
            stat.running = rest.first().and_then(|v| v.parse().ok()).unwrap_or(0);
        }
    }
    stat
}

/// Busy fraction (0–100) between two readings.
pub fn busy(prev: &Times, now: &Times) -> (f64, f64, f64) {
    let total = now.total.saturating_sub(prev.total) as f64;
    if total <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let idle = now.idle.saturating_sub(prev.idle) as f64;
    let user = now.user.saturating_sub(prev.user) as f64;
    let system = now.system.saturating_sub(prev.system) as f64;
    (((total - idle) / total * 100.0).clamp(0.0, 100.0), user / total * 100.0, system / total * 100.0)
}

#[derive(Debug, Clone, Default)]
pub struct Cpu {
    pub model: String,
    /// Busy % of all cores together.
    pub usage: f64,
    pub user: f64,
    pub system: f64,
    pub cores: Vec<f64>,
    /// Current frequency of each core, MHz.
    pub freqs: Vec<f64>,
    pub max_freq: f64,
    pub governor: String,
    pub load: [f64; 3],
    pub uptime: f64,
    pub ctxt_per_sec: f64,
    pub running: u64,
    /// Physical cores (distinct core ids), when known.
    pub physical: usize,
}

pub struct CpuSampler {
    prev: Option<Stat>,
    model: String,
    physical: usize,
    max_freq: f64,
}

impl CpuSampler {
    pub fn new() -> Self {
        let info = read("/proc/cpuinfo");
        let model = info
            .lines()
            .find(|l| l.starts_with("model name"))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_else(|| "Processor".into());
        let mut ids: Vec<(String, String)> = Vec::new();
        let (mut phys, mut core) = (String::new(), String::new());
        for line in info.lines() {
            if let Some((k, v)) = line.split_once(':') {
                match k.trim() {
                    "physical id" => phys = v.trim().into(),
                    "core id" => core = v.trim().into(),
                    _ => {}
                }
            } else if !core.is_empty() {
                ids.push((phys.clone(), core.clone()));
                core.clear();
            }
        }
        ids.sort();
        ids.dedup();
        let max_freq =
            read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq").trim().parse::<f64>().unwrap_or(0.0) / 1000.0;
        CpuSampler { prev: None, model, physical: ids.len(), max_freq }
    }

    pub fn sample(&mut self, elapsed: f64) -> Cpu {
        let stat = parse_stat(&read("/proc/stat"));
        let mut cpu = Cpu {
            model: self.model.clone(),
            physical: self.physical,
            max_freq: self.max_freq,
            running: stat.running,
            ..Default::default()
        };
        if let Some(prev) = &self.prev {
            (cpu.usage, cpu.user, cpu.system) = busy(&prev.total, &stat.total);
            cpu.cores = stat.cores.iter().zip(&prev.cores).map(|(n, p)| busy(p, n).0).collect();
            if elapsed > 0.0 {
                cpu.ctxt_per_sec = stat.ctxt.saturating_sub(prev.ctxt) as f64 / elapsed;
            }
        } else {
            cpu.cores = vec![0.0; stat.cores.len()];
        }
        cpu.freqs = (0..stat.cores.len())
            .map(|i| {
                read(&format!("/sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq")).trim().parse::<f64>().unwrap_or(0.0)
                    / 1000.0
            })
            .collect();
        cpu.governor = read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").trim().to_string();
        let load = read("/proc/loadavg");
        for (i, v) in load.split_whitespace().take(3).enumerate() {
            cpu.load[i] = v.parse().unwrap_or(0.0);
        }
        cpu.uptime = read("/proc/uptime").split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        self.prev = Some(stat);
        cpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "cpu  100 0 50 800 50 0 0 0 0 0\n\
                        cpu0 50 0 25 400 25 0 0 0 0 0\n\
                        cpu1 50 0 25 400 25 0 0 0 0 0\n\
                        intr 1 2 3\nctxt 5000\nbtime 1\nprocs_running 3\n";

    #[test]
    fn parses_proc_stat() {
        let s = parse_stat(STAT);
        assert_eq!(s.cores.len(), 2);
        assert_eq!(s.total.total, 1000);
        assert_eq!(s.total.idle, 850);
        assert_eq!(s.ctxt, 5000);
        assert_eq!(s.running, 3);
    }

    #[test]
    fn busy_between_readings() {
        let a = Times { user: 0, system: 0, idle: 0, total: 0 };
        let b = Times { user: 30, system: 10, idle: 60, total: 100 };
        let (usage, user, system) = busy(&a, &b);
        assert!((usage - 40.0).abs() < 1e-9);
        assert!((user - 30.0).abs() < 1e-9);
        assert!((system - 10.0).abs() < 1e-9);
    }
}
