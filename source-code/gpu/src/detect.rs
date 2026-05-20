use std::path::{Path, PathBuf};
use anyhow::Result;
use tracing::{debug, warn};
use crate::{GpuCapabilities, GpuVendor};

const VID_AMD:    u16 = 0x1002;
const VID_NVIDIA: u16 = 0x10de;
const VID_INTEL:  u16 = 0x8086;

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub name:        String,
    pub vendor:      GpuVendor,
    pub vendor_id:   u16,
    pub device_id:   u16,
    pub driver:      String,
    pub drm_node:    PathBuf,   // /dev/dri/cardN
    pub render_node: Option<String>, // /dev/dri/renderDN
    pub caps:        GpuCapabilities,
}

/// Detect primary (best) GPU.
pub fn detect_primary() -> Result<Option<GpuInfo>> {
    Ok(detect_all()?.into_iter().next())
}

/// Detect ALL supported GPUs, sorted by desirability.
pub fn detect_all() -> Result<Vec<GpuInfo>> {
    let mut results = Vec::new();
    let pci_dir = Path::new("/sys/bus/pci/devices");
    if !pci_dir.exists() {
        warn!("No /sys/bus/pci – container environment? GPU detection skipped.");
        return Ok(results);
    }

    for entry in std::fs::read_dir(pci_dir)? {
        let path = entry?.path();

        // Only display-class PCI devices (0x03xxxx)
        let class_raw = read_sysfs_str(path.join("class")).unwrap_or_default();
        if !class_raw.starts_with("0x03") { continue; }

        let vendor_id = parse_hex_u16(path.join("vendor"));
        let device_id = parse_hex_u16(path.join("device"));

        let vendor = match vendor_id {
            VID_AMD    => GpuVendor::Amd,
            VID_NVIDIA => GpuVendor::Nvidia,
            VID_INTEL  => GpuVendor::Intel,
            _          => continue,
        };

        let driver     = read_driver_name(&path);
        let drm_node   = find_drm_node(&path).unwrap_or_else(|| PathBuf::from("/dev/dri/card0"));
        let render_node = find_render_node(&path);
        let name       = gpu_display_name(&path, vendor_id, device_id, &vendor);
        let caps       = probe_caps(&vendor, &driver, &drm_node);

        debug!(%name, %vendor, %driver, ?drm_node, "GPU found");
        results.push(GpuInfo { name, vendor, vendor_id, device_id, driver, drm_node, render_node, caps });
    }

    // Sort: AMD open-source first (best legacy support), then Intel, Nvidia
    results.sort_by_key(|g| match g.vendor {
        GpuVendor::Amd      => 0u8,
        GpuVendor::Intel    => 1,
        GpuVendor::Nvidia   => 2,
        GpuVendor::Software => 3,
    });

    Ok(results)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn read_sysfs_str(p: PathBuf) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn parse_hex_u16(p: PathBuf) -> u16 {
    read_sysfs_str(p)
        .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0)
}

fn read_driver_name(pci_path: &Path) -> String {
    let link = pci_path.join("driver");
    std::fs::read_link(link)
        .ok()
        .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "none".into())
}

fn find_drm_node(pci_path: &Path) -> Option<PathBuf> {
    let drm_dir = pci_path.join("drm");
    if !drm_dir.exists() { return None; }
    std::fs::read_dir(drm_dir).ok()?.find_map(|e| {
        let e = e.ok()?;
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("card") {
            Some(PathBuf::from(format!("/dev/dri/{name}")))
        } else { None }
    })
}

fn find_render_node(pci_path: &Path) -> Option<String> {
    let drm_dir = pci_path.join("drm");
    if !drm_dir.exists() { return None; }
    std::fs::read_dir(drm_dir).ok()?.find_map(|e| {
        let e = e.ok()?;
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("render") {
            Some(format!("/dev/dri/{name}"))
        } else { None }
    })
}

fn gpu_display_name(pci_path: &Path, vid: u16, did: u16, vendor: &GpuVendor) -> String {
    // Try /sys label first
    if let Some(l) = read_sysfs_str(pci_path.join("label")) {
        return l;
    }
    // Friendly fallback based on well-known DIDs
    match (vid, did) {
        // AMD GCN1 – HD 7000
        (VID_AMD, 0x6798..=0x679E) => "AMD Radeon HD 7970/7950".into(),
        // AMD GCN2 – R9 290X
        (VID_AMD, 0x67B0 | 0x67B1) => "AMD Radeon R9 290X/290".into(),
        // AMD GCN3 – R9 380/385
        (VID_AMD, 0x6939 | 0x693B) => "AMD Radeon R9 380/385".into(),
        // AMD Polaris – RX 480/580
        (VID_AMD, 0x67DF) => "AMD Radeon RX 480/580".into(),
        // Nvidia Maxwell – GTX 960
        (VID_NVIDIA, 0x1401) => "NVIDIA GeForce GTX 960".into(),
        // Nvidia Maxwell – GTX 970
        (VID_NVIDIA, 0x13C2) => "NVIDIA GeForce GTX 970".into(),
        // Nvidia Pascal – GTX 1060
        (VID_NVIDIA, 0x1C03) => "NVIDIA GeForce GTX 1060".into(),
        // Nvidia Pascal – GTX 1070
        (VID_NVIDIA, 0x1B81) => "NVIDIA GeForce GTX 1070".into(),
        // Intel Skylake HD 530
        (VID_INTEL, 0x1912) => "Intel HD Graphics 530 (Skylake)".into(),
        // Intel Kaby Lake HD 620
        (VID_INTEL, 0x5916) => "Intel HD Graphics 620 (Kaby Lake)".into(),
        // Intel Coffee Lake UHD 630
        (VID_INTEL, 0x3E92 | 0x3E9B) => "Intel UHD Graphics 630 (Coffee Lake)".into(),
        // Intel Whiskey Lake UHD 620
        (VID_INTEL, 0x3EA0) => "Intel UHD Graphics 620 (Whiskey Lake)".into(),
        // Intel Ice Lake Iris Plus
        (VID_INTEL, 0x8A52 | 0x8A56) => "Intel Iris Plus Graphics (Ice Lake)".into(),
        // Intel Alder Lake UHD 770
        (VID_INTEL, 0x4680) => "Intel UHD Graphics 770 (Alder Lake)".into(),
        _ => format!("{vendor} GPU [{vid:04x}:{did:04x}]"),
    }
}

fn probe_caps(vendor: &GpuVendor, driver: &str, _drm_node: &Path) -> GpuCapabilities {
    match vendor {
        GpuVendor::Amd => GpuCapabilities {
            vrr:          true,
            hdr:          driver == "amdgpu",        // Only AMDGPU DC has HDR planes
            atomic_kms:   driver == "amdgpu",
            dmabuf:       true,
            prime:        true,
            vulkan:       true,  // Mesa RADV
            hw_cursor:    true,
            overlay_plane: driver == "amdgpu",
        },
        GpuVendor::Nvidia => GpuCapabilities {
            vrr:          driver == "nvidia",         // G-Sync only on prop driver
            hdr:          driver == "nvidia",
            atomic_kms:   check_nvidia_kms(),
            dmabuf:       driver == "nvidia",
            prime:        driver == "nvidia",
            vulkan:       true,  // nouveau-nv50 or nvidia
            hw_cursor:    true,
            overlay_plane: false,                     // Nouveau has no overlay plane
        },
        GpuVendor::Intel => GpuCapabilities {
            vrr:          true,   // PSR2 / Adaptive Sync on Gen11+
            hdr:          false,  // HDR support incomplete in i915 for these gens
            atomic_kms:   true,
            dmabuf:       true,
            prime:        true,
            vulkan:       true,   // Intel ANV (Vulkan)
            hw_cursor:    true,
            overlay_plane: true,
        },
        GpuVendor::Software => GpuCapabilities::default(),
    }
}

fn check_nvidia_kms() -> bool {
    std::fs::read_to_string("/sys/module/nvidia_drm/parameters/modeset")
        .map(|v| v.trim() == "Y")
        .unwrap_or(false)
}
