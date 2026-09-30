//! Per-disk throughput from `/proc/diskstats`, and mounted filesystems with usage.

use super::read;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Counters {
    pub sectors_read: u64,
    pub sectors_written: u64,
    pub io_ms: u64,
}

/// `/proc/diskstats` counters by device name.
pub fn parse_diskstats(text: &str) -> HashMap<String, Counters> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 13 {
                return None;
            }
            let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
            Some((f[2].to_string(), Counters { sectors_read: n(5), sectors_written: n(9), io_ms: n(12) }))
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct Mount {
    pub device: String,
    pub path: String,
    pub fs: String,
    pub total: u64,
    pub free: u64,
}

impl Mount {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Disk {
    pub name: String,
    pub model: String,
    pub size: u64,
    pub rotational: bool,
    pub removable: bool,
    pub read_bps: f64,
    pub write_bps: f64,
    /// % of the interval the device was busy.
    pub busy: f64,
    pub read_total: u64,
    pub written_total: u64,
    pub temp: Option<f64>,
    pub mounts: Vec<Mount>,
}

/// Whole disks we show: real block devices, not partitions, loops, ramdisks or zram.
fn disks() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir("/sys/block")
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    names.retain(|n| {
        !(n.starts_with("loop") || n.starts_with("ram") || n.starts_with("zram") || n.starts_with("dm-") || n.starts_with("sr"))
            && Path::new(&format!("/sys/block/{n}/device")).exists()
    });
    names.sort();
    names
}

/// The whole disks a block device (partition, dm-crypt, LVM…) ultimately lives on.
pub fn parent_disks(dev: &str, known: &[String]) -> Vec<String> {
    if known.iter().any(|k| k == dev) {
        return vec![dev.to_string()];
    }
    let sys = PathBuf::from("/sys/class/block").join(dev);
    let slaves: Vec<String> = std::fs::read_dir(sys.join("slaves"))
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    if !slaves.is_empty() {
        let mut out: Vec<String> = slaves.iter().flat_map(|s| parent_disks(s, known)).collect();
        out.dedup();
        return out;
    }
    // A partition's sysfs directory sits inside its disk's.
    if let Ok(real) = std::fs::canonicalize(&sys)
        && let Some(parent) = real.parent().and_then(|p| p.file_name())
    {
        let p = parent.to_string_lossy().to_string();
        if known.contains(&p) {
            return vec![p];
        }
    }
    vec![]
}

pub fn parse_mounts(text: &str) -> Vec<(String, String, String)> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<(String, String, String)> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 || !f[0].starts_with("/dev/") || f[0].starts_with("/dev/loop") {
            continue;
        }
        let path = f[1].replace("\\040", " ");
        // btrfs subvolumes share a device; keep the shortest mount point.
        if let Some(&i) = seen.get(f[0]) {
            if path.len() < out[i].1.len() {
                out[i].1 = path;
            }
            continue;
        }
        seen.insert(f[0].to_string(), out.len());
        out.push((f[0].to_string(), path, f[2].to_string()));
    }
    out
}

fn statvfs(path: &str) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let frsize = s.f_frsize as u64;
    Some((s.f_blocks as u64 * frsize, s.f_bavail as u64 * frsize))
}

fn nvme_temp(disk: &str) -> Option<f64> {
    let dev = PathBuf::from(format!("/sys/block/{disk}/device"));
    let hw = std::fs::read_dir(&dev).ok()?.flatten().find(|e| e.file_name().to_string_lossy().starts_with("hwmon"))?;
    read(&hw.path().join("temp1_input").to_string_lossy()).trim().parse::<f64>().ok().map(|v| v / 1000.0)
}

pub struct DiskSampler {
    prev: HashMap<String, Counters>,
    names: Vec<String>,
    info: HashMap<String, (String, u64, bool, bool)>,
    tick: u64,
    mounts: Vec<(String, String, String, Vec<String>)>,
    temps: HashMap<String, Option<f64>>,
}

impl DiskSampler {
    pub fn new() -> Self {
        DiskSampler {
            prev: HashMap::new(),
            names: Vec::new(),
            info: HashMap::new(),
            tick: 0,
            mounts: Vec::new(),
            temps: HashMap::new(),
        }
    }

    fn refresh(&mut self) {
        self.names = disks();
        for n in &self.names {
            self.info.entry(n.clone()).or_insert_with(|| {
                let base = format!("/sys/block/{n}");
                let model = read(&format!("{base}/device/model")).trim().to_string();
                let size = read(&format!("{base}/size")).trim().parse::<u64>().unwrap_or(0) * 512;
                let rota = read(&format!("{base}/queue/rotational")).trim() == "1";
                let removable = read(&format!("{base}/removable")).trim() == "1";
                (if model.is_empty() { n.clone() } else { model }, size, rota, removable)
            });
        }
        self.mounts = parse_mounts(&read("/proc/mounts"))
            .into_iter()
            .map(|(dev, path, fs)| {
                let real = std::fs::canonicalize(&dev).unwrap_or_else(|_| PathBuf::from(&dev));
                let name = real.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let parents = parent_disks(&name, &self.names);
                (dev, path, fs, parents)
            })
            .collect();
        for n in &self.names {
            self.temps.insert(n.clone(), nvme_temp(n));
        }
    }

    pub fn sample(&mut self, elapsed: f64) -> Vec<Disk> {
        // Devices and mounts change rarely; look again every few seconds.
        if self.tick.is_multiple_of(5) {
            self.refresh();
        }
        self.tick += 1;
        let stats = parse_diskstats(&read("/proc/diskstats"));
        let mut out = Vec::new();
        for n in &self.names {
            let Some(now) = stats.get(n) else { continue };
            let (model, size, rotational, removable) = self.info.get(n).cloned().unwrap_or_default();
            let mut d = Disk {
                name: n.clone(),
                model,
                size,
                rotational,
                removable,
                read_total: now.sectors_read * 512,
                written_total: now.sectors_written * 512,
                temp: self.temps.get(n).copied().flatten(),
                ..Default::default()
            };
            if let Some(prev) = self.prev.get(n)
                && elapsed > 0.0
            {
                d.read_bps = now.sectors_read.saturating_sub(prev.sectors_read) as f64 * 512.0 / elapsed;
                d.write_bps = now.sectors_written.saturating_sub(prev.sectors_written) as f64 * 512.0 / elapsed;
                d.busy = (now.io_ms.saturating_sub(prev.io_ms) as f64 / (elapsed * 1000.0) * 100.0).clamp(0.0, 100.0);
            }
            d.mounts = self
                .mounts
                .iter()
                .filter(|m| m.3.contains(n))
                .filter_map(|(dev, path, fs, _)| {
                    let (total, free) = statvfs(path)?;
                    Some(Mount { device: dev.clone(), path: path.clone(), fs: fs.clone(), total, free })
                })
                .collect();
            out.push(d);
        }
        self.prev = stats;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_diskstats() {
        let text = " 259       0 nvme0n1 473 0 27469 62 10 0 800 5 0 33 62 0 0 0 0 0 0\n";
        let s = parse_diskstats(text);
        let c = s["nvme0n1"];
        assert_eq!(c.sectors_read, 27469);
        assert_eq!(c.sectors_written, 800);
        assert_eq!(c.io_ms, 33);
    }

    #[test]
    fn dedups_btrfs_subvolumes() {
        let text = "/dev/mapper/root /home btrfs rw 0 0\n/dev/mapper/root / btrfs rw 0 0\n\
                    /dev/nvme0n1p1 /boot vfat rw 0 0\ntmpfs /tmp tmpfs rw 0 0\n";
        let m = parse_mounts(text);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].1, "/");
        assert_eq!(m[1].1, "/boot");
    }
}
