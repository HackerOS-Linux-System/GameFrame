use std::fs;
use gameframe_overlay::Telemetry;
use tracing::trace;

/// Read all telemetry for the given DRM card (e.g. "card0").
pub fn read_telemetry(drm_card: &str) -> Telemetry {
    Telemetry {
        fps:       0.0, // filled by render loop
        frame_ms:  0.0,
        gpu_temp:  read_gpu_temp(drm_card),
        gpu_usage: read_gpu_usage(drm_card),
        cpu_usage: read_cpu_usage(),
        ram_used:  read_ram_used(),
        ram_total: read_ram_total(),
        vram_used: read_vram_used(drm_card),
    }
}

// ── GPU temperature ───────────────────────────────────────────────────────────
// /sys/class/drm/<card>/device/hwmon/hwmonN/temp1_input (millidegrees C)

fn read_gpu_temp(card: &str) -> Option<u32> {
    let base = format!("/sys/class/drm/{card}/device/hwmon");
    for entry in fs::read_dir(&base).ok()? {
        let entry = entry.ok()?;
        let temp_path = entry.path().join("temp1_input");
        if let Ok(raw) = fs::read_to_string(&temp_path) {
            if let Ok(mdeg) = raw.trim().parse::<i64>() {
                let deg = (mdeg / 1000) as u32;
                trace!(%card, gpu_temp = deg, "GPU temp");
                return Some(deg);
            }
        }
    }
    None
}

// ── GPU usage ─────────────────────────────────────────────────────────────────
// AMD: /sys/class/drm/<card>/device/gpu_busy_percent
// Intel: gt_cur_freq_mhz / gt_max_freq_mhz as proxy

fn read_gpu_usage(card: &str) -> Option<u32> {
    let amd = format!("/sys/class/drm/{card}/device/gpu_busy_percent");
    if let Ok(raw) = fs::read_to_string(&amd) {
        if let Ok(pct) = raw.trim().parse::<u32>() {
            return Some(pct);
        }
    }
    let cur_path = format!("/sys/class/drm/{card}/device/gt_cur_freq_mhz");
    let max_path = format!("/sys/class/drm/{card}/device/gt_max_freq_mhz");
    if let (Ok(cur), Ok(max)) = (fs::read_to_string(&cur_path), fs::read_to_string(&max_path)) {
        if let (Ok(c), Ok(m)) = (cur.trim().parse::<u32>(), max.trim().parse::<u32>()) {
            if m > 0 { return Some((c * 100) / m); }
        }
    }
    None
}

// ── VRAM usage (AMD) ──────────────────────────────────────────────────────────

fn read_vram_used(card: &str) -> Option<u64> {
    let path = format!("/sys/class/drm/{card}/device/mem_info_vram_used");
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|b| b / 1024 / 1024)
}

// ── CPU usage ─────────────────────────────────────────────────────────────────

fn read_cpu_usage() -> Option<f32> {
    use std::cell::Cell;
    thread_local! {
        static PREV: Cell<Option<(u64, u64)>> = Cell::new(None);
    }

    let raw = fs::read_to_string("/proc/stat").ok()?;
    let line = raw.lines().next()?;
    let fields: Vec<u64> = line.split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if fields.len() < 4 { return None; }

    let idle  = fields[3] + fields.get(4).copied().unwrap_or(0);
    let total: u64 = fields.iter().sum();

    PREV.with(|prev| {
        let result = if let Some((p_idle, p_total)) = prev.get() {
            let d_idle  = idle.saturating_sub(p_idle);
            let d_total = total.saturating_sub(p_total);
            if d_total == 0 { None }
            else { Some(((d_total - d_idle) as f32 / d_total as f32) * 100.0) }
        } else { None };
        prev.set(Some((idle, total)));
        result
    })
}

// ── RAM ───────────────────────────────────────────────────────────────────────

fn parse_meminfo_kb(key: &str) -> Option<u64> {
    let raw = fs::read_to_string("/proc/meminfo").ok()?;
    raw.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map(|kb| kb / 1024)
}

fn read_ram_total() -> Option<u64> { parse_meminfo_kb("MemTotal:") }

fn read_ram_used() -> Option<u64> {
    let total   = parse_meminfo_kb("MemTotal:")?;
    let free    = parse_meminfo_kb("MemFree:").unwrap_or(0);
    let buffers = parse_meminfo_kb("Buffers:").unwrap_or(0);
    let cached  = parse_meminfo_kb("Cached:").unwrap_or(0);
    let srec    = parse_meminfo_kb("SReclaimable:").unwrap_or(0);
    Some(total.saturating_sub(free + buffers + cached + srec))
}
