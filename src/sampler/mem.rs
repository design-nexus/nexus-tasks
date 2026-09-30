//! Memory, swap and zram from `/proc/meminfo` and `/sys/block/zram*`.

use super::read;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Mem {
    pub total: u64,
    pub available: u64,
    pub used: u64,
    pub cached: u64,
    pub buffers: u64,
    pub shared: u64,
    pub dirty: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Uncompressed data held in zram, and the RAM it actually takes.
    pub zram_data: u64,
    pub zram_used: u64,
}

/// `/proc/meminfo` as bytes by key.
pub fn parse_meminfo(text: &str) -> HashMap<String, u64> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            let mut parts = v.split_whitespace();
            let n: u64 = parts.next()?.parse().ok()?;
            let mult = if parts.next() == Some("kB") { 1024 } else { 1 };
            Some((k.to_string(), n * mult))
        })
        .collect()
}

pub fn from_meminfo(info: &HashMap<String, u64>) -> Mem {
    let g = |k: &str| info.get(k).copied().unwrap_or(0);
    let total = g("MemTotal");
    let available = if info.contains_key("MemAvailable") { g("MemAvailable") } else { g("MemFree") + g("Cached") };
    Mem {
        total,
        available,
        used: total.saturating_sub(available),
        cached: (g("Cached") + g("SReclaimable")).saturating_sub(g("Shmem")),
        buffers: g("Buffers"),
        shared: g("Shmem"),
        dirty: g("Dirty"),
        swap_total: g("SwapTotal"),
        swap_used: g("SwapTotal").saturating_sub(g("SwapFree")),
        zram_data: 0,
        zram_used: 0,
    }
}

pub fn sample() -> Mem {
    let mut mem = from_meminfo(&parse_meminfo(&read("/proc/meminfo")));
    if let Ok(entries) = std::fs::read_dir("/sys/block") {
        for e in entries.flatten() {
            if !e.file_name().to_string_lossy().starts_with("zram") {
                continue;
            }
            // orig_data_size compr_data_size mem_used_total …
            let stat = read(&e.path().join("mm_stat").to_string_lossy());
            let n: Vec<u64> = stat.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            if n.len() >= 3 {
                mem.zram_data += n[0];
                mem.zram_used += n[2];
            }
        }
    }
    mem
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meminfo() {
        let text = "MemTotal:       16000 kB\nMemFree:  1000 kB\nMemAvailable:   6000 kB\nBuffers: 100 kB\n\
                    Cached: 4000 kB\nSReclaimable: 500 kB\nShmem: 300 kB\nSwapTotal: 8000 kB\nSwapFree: 7000 kB\n\
                    HugePages_Total: 0\n";
        let m = from_meminfo(&parse_meminfo(text));
        assert_eq!(m.total, 16000 * 1024);
        assert_eq!(m.used, 10000 * 1024);
        assert_eq!(m.cached, 4200 * 1024);
        assert_eq!(m.swap_used, 1000 * 1024);
    }
}
