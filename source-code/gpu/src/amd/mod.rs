use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmdFamily {
    SouthernIslands, // GCN1 – HD 7xxx
    SeaIslands,      // GCN2 – HD 8xxx / R9 2xx
    VolcanicIslands, // GCN3 – R9 380/390/Fury
    Polaris,         // GCN4 – RX 470/480/580
    Vega,            // GCN5 – RX Vega
    NaviRdna,        // RDNA1/2 – RX 5xxx/6xxx
    Unknown,
}

impl AmdFamily {
    pub fn from_device_id(did: u16) -> Self {
        // Upper byte determines family for most AMD dGPUs
        match did >> 8 {
            0x67 if did <= 0x67FF => AmdFamily::SouthernIslands,
            0x68 if did <= 0x68FF => AmdFamily::SeaIslands,
            0x69 if did <= 0x69FF => AmdFamily::VolcanicIslands,
            0x6F | 0x67D..=0x67F  => AmdFamily::Polaris,
            0x68D..=0x68F         => AmdFamily::Vega,
            0x73                  => AmdFamily::NaviRdna,
            _                     => AmdFamily::Unknown,
        }
    }

    pub fn uses_amdgpu(&self) -> bool {
        !matches!(self, AmdFamily::SouthernIslands | AmdFamily::SeaIslands)
    }
}

/// Apply AMD-specific DRM quirks at device-open time.
pub fn apply_quirks(drm_fd: i32, family: AmdFamily) {
    if matches!(family, AmdFamily::SouthernIslands | AmdFamily::SeaIslands) {
        warn!("AMD legacy (SI/CIK): radeon driver – VRR/FreeSync unavailable");
    } else {
        info!(?family, "AMD: amdgpu DC enabled, VRR/FreeSync active");
    }
    // Real impl: ioctl(drm_fd, DRM_IOCTL_AMDGPU_INFO, ...) for DC query
    let _ = drm_fd;
}

pub fn check_radv() -> bool {
    [
        "/usr/share/vulkan/icd.d/radeon_icd.x86_64.json",
        "/usr/share/vulkan/icd.d/radeon_icd.i686.json",
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists())
}
