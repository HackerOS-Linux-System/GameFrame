pub mod amd;
pub mod detect;
pub mod intel;
pub mod nvidia;

pub use detect::{detect_all, detect_primary, GpuInfo};

// ── Vendor enum ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Amd,
    Nvidia,
    Intel,
    Software,
}

impl std::fmt::Display for GpuVendor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Amd      => write!(f, "AMD"),
            Self::Nvidia   => write!(f, "NVIDIA"),
            Self::Intel    => write!(f, "Intel"),
            Self::Software => write!(f, "Software (llvmpipe)"),
        }
    }
}

/// Capabilities reported after probing a GPU.
#[derive(Debug, Clone, Default)]
pub struct GpuCapabilities {
    pub vrr:          bool,  // FreeSync / G-Sync / Adaptive-Sync
    pub hdr:          bool,  // HDR10 metadata plane
    pub atomic_kms:   bool,  // DRM atomic modesetting
    pub dmabuf:       bool,  // linux-dmabuf
    pub prime:        bool,  // PRIME (multi-GPU/dGPU+iGPU)
    pub vulkan:       bool,  // Vulkan ICD found
    pub hw_cursor:    bool,  // Hardware cursor plane
    pub overlay_plane: bool, // Overlay DRM plane (for gameframe UI)
}

/// Print human-readable GPU info (used by `gameframe gpu-info`).
pub fn print_gpu_info() -> anyhow::Result<()> {
    let gpus = detect_all()?;
    if gpus.is_empty() {
        println!("No supported GPUs detected.");
        return Ok(());
    }
    for gpu in &gpus {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("  GPU     : {}", gpu.name);
        println!("  Vendor  : {}", gpu.vendor);
        println!("  PCI     : {:04x}:{:04x}", gpu.vendor_id, gpu.device_id);
        println!("  Driver  : {}", gpu.driver);
        println!("  DRM node: {}", gpu.drm_node.display());
        println!("  Render  : {}", gpu.render_node.as_deref().unwrap_or("n/a").to_string());
        println!("  Caps    :");
        println!("    VRR          : {}", gpu.caps.vrr);
        println!("    HDR          : {}", gpu.caps.hdr);
        println!("    Atomic KMS   : {}", gpu.caps.atomic_kms);
        println!("    DMABUF       : {}", gpu.caps.dmabuf);
        println!("    PRIME        : {}", gpu.caps.prime);
        println!("    Vulkan       : {}", gpu.caps.vulkan);
        println!("    HW cursor    : {}", gpu.caps.hw_cursor);
        println!("    Overlay plane: {}", gpu.caps.overlay_plane);
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    Ok(())
}
